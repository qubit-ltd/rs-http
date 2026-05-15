/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! # HTTP Logger
//!
//! Encapsulates request and response logging behavior.
//!

use http::header::CONTENT_TYPE;

use crate::{
    BodyLogContext,
    BodyPreview,
    HttpClientOptions,
    HttpLoggingOptions,
    HttpRequest,
    HttpRequestBody,
    HttpResponse,
    HttpResponseMeta,
    LogSanitizer,
};

const UNRESOLVED_REQUEST_URL: &str = "<unresolved request URL>";
const STREAMING_REQUEST_BODY_SKIPPED: &str = "<skipped: streaming request body>";

/// HTTP logger bound to one pair of logging options and a sanitizer policy.
#[derive(Debug, Clone)]
pub struct HttpLogger<'a> {
    options: &'a HttpLoggingOptions,
    sanitizer: LogSanitizer,
}

/// Request body preview category used by TRACE logging.
enum RequestBodyLogPreview<'a> {
    /// Borrowed bytes that can be safely previewed without consuming a stream.
    Bytes(&'a [u8]),
    /// No request body is present.
    Empty,
    /// Body logging is intentionally skipped.
    Skipped(&'static str),
}

impl<'a> HttpLogger<'a> {
    /// Creates a logger view from one client option object.
    ///
    /// # Parameters
    /// - `options`: Client options that carry logging switches and sensitive
    ///   header policies.
    ///
    /// # Returns
    /// A logger that emits TRACE records according to the provided options.
    pub fn new(options: &'a HttpClientOptions) -> Self {
        Self {
            options: &options.logging,
            sanitizer: LogSanitizer::new(options.log_sanitize_policy.clone()),
        }
    }

    /// Emits TRACE logs for an outbound request when logging is enabled and TRACE is active.
    ///
    /// # Parameters
    /// - `request`: Prepared request snapshot; expected to carry resolved URL
    ///   and attempt-level merged headers.
    ///
    /// # Returns
    /// Nothing; no-op when disabled or TRACE off.
    pub fn log_request(&self, request: &HttpRequest) {
        if !self.is_trace_enabled() {
            return;
        }

        let url = self.request_log_url(request);
        tracing::trace!("--> {} {}", request.method(), url);

        let headers = request
            .effective_headers_cached()
            .unwrap_or_else(|| request.headers());

        if self.options.log_request_header {
            for (name, value) in headers {
                let masked = self.sanitizer.sanitize_header_value(name, value);
                tracing::trace!("{}: {}", name.as_str(), masked);
            }
        }

        if self.options.log_request_body {
            match Self::request_body_for_log(request) {
                RequestBodyLogPreview::Bytes(bytes) => {
                    let content_type = Self::content_type(headers);
                    tracing::trace!(
                        "Request body: {}",
                        self.render_body(bytes, BodyLogContext::Request, content_type)
                    );
                }
                RequestBodyLogPreview::Empty => tracing::trace!("Request body: <empty>"),
                RequestBodyLogPreview::Skipped(reason) => tracing::trace!("Request body: {reason}"),
            }
        }
    }

    /// Emits TRACE logs for a completed response (headers and optional body preview).
    ///
    /// # Parameters
    /// - `response`: Response object (status/url/headers/body cache).
    ///
    /// # Returns
    /// `Ok(())` on success; no-op when disabled or TRACE off.
    ///
    /// # Errors
    /// Returns [`crate::HttpError`] when reading the response body for logging fails.
    pub async fn log_response(&self, response: &mut HttpResponse) -> crate::HttpResult<()> {
        if !self.is_trace_enabled() {
            return Ok(());
        }

        tracing::trace!("<-- {} {}", response.status().as_u16(), response.url());

        if self.options.log_response_header {
            for (name, value) in response.headers() {
                let masked = self.sanitizer.sanitize_header_value(name, value);
                tracing::trace!("{}: {}", name.as_str(), masked);
            }
        }

        if self.options.log_response_body {
            let content_type = Self::content_type(response.headers()).map(str::to_string);
            if let Some(body) = response.buffered_body_for_logging() {
                tracing::trace!(
                    "Response body: {}",
                    self.render_body(
                        body.as_ref(),
                        BodyLogContext::Response,
                        content_type.as_deref()
                    )
                );
            } else if response.can_buffer_body_for_logging(self.options.body_size_limit) {
                let body = response.bytes().await?;
                tracing::trace!(
                    "Response body: {}",
                    self.render_body(
                        body.as_ref(),
                        BodyLogContext::Response,
                        content_type.as_deref()
                    )
                );
            } else {
                tracing::trace!("Response body: <skipped: streaming or unknown-size body>");
            }
        }
        Ok(())
    }

    /// Logs response line and headers for a streaming call without reading the body stream.
    ///
    /// # Parameters
    /// - `response_meta`: Response metadata (status/url/headers).
    ///
    /// # Returns
    /// Nothing; no-op when disabled or TRACE off.
    pub fn log_stream_response_headers(&self, response_meta: &HttpResponseMeta) {
        if !self.is_trace_enabled() {
            return;
        }

        tracing::trace!(
            "<-- {} {} (stream)",
            response_meta.status().as_u16(),
            response_meta.url()
        );

        if self.options.log_response_header {
            for (name, value) in response_meta.headers() {
                let masked = self.sanitizer.sanitize_header_value(name, value);
                tracing::trace!("{}: {}", name.as_str(), masked);
            }
        }
    }

    /// Returns whether TRACE logs should be emitted under current options and subscriber state.
    ///
    /// # Returns
    /// `true` when logging is enabled and TRACE is active.
    pub fn is_trace_enabled(&self) -> bool {
        self.options.enabled && tracing::enabled!(tracing::Level::TRACE)
    }

    /// Returns the URL text used by request logging.
    ///
    /// # Parameters
    /// - `request`: Request whose resolved URL should be rendered.
    ///
    /// # Returns
    /// Resolved URL including builder query parameters, or a fixed placeholder
    /// when URL resolution fails before send.
    fn request_log_url(&self, request: &HttpRequest) -> String {
        request
            .resolved_url()
            .map(|url| self.sanitizer.sanitize_url(&url))
            .unwrap_or_else(|_| UNRESOLVED_REQUEST_URL.to_string())
    }

    /// Formats up to configured `body_size_limit` bytes of `body` for TRACE output.
    ///
    /// # Parameters
    /// - `body`: Raw bytes.
    /// - `context`: Body logging call site.
    /// - `content_type`: Optional Content-Type value.
    ///
    /// # Returns
    /// Human-readable sanitized body preview string.
    fn render_body(
        &self,
        body: &[u8],
        context: BodyLogContext,
        content_type: Option<&str>,
    ) -> String {
        let preview = BodyPreview::new(body, self.options.body_size_limit, context);
        let preview = if let Some(content_type) = content_type {
            preview.with_content_type(content_type)
        } else {
            preview
        };
        self.sanitizer.sanitize_body_preview(&preview)
    }

    /// Borrows request body content only when body logging is safe.
    ///
    /// # Parameters
    /// - `request`: Prepared request snapshot.
    ///
    /// # Returns
    /// Body preview category for logger rendering.
    fn request_body_for_log(request: &HttpRequest) -> RequestBodyLogPreview<'_> {
        if request.has_streaming_body() {
            return RequestBodyLogPreview::Skipped(STREAMING_REQUEST_BODY_SKIPPED);
        }
        match request.body() {
            HttpRequestBody::Bytes(bytes)
            | HttpRequestBody::Json(bytes)
            | HttpRequestBody::Form(bytes)
            | HttpRequestBody::Multipart(bytes)
            | HttpRequestBody::Ndjson(bytes) => RequestBodyLogPreview::Bytes(bytes.as_ref()),
            HttpRequestBody::Text(text) => RequestBodyLogPreview::Bytes(text.as_bytes()),
            HttpRequestBody::Stream(_) => {
                RequestBodyLogPreview::Skipped(STREAMING_REQUEST_BODY_SKIPPED)
            }
            HttpRequestBody::Empty => RequestBodyLogPreview::Empty,
        }
    }

    /// Extracts a UTF-8 Content-Type header value from a header map.
    ///
    /// # Parameters
    /// - `headers`: Headers to inspect.
    ///
    /// # Returns
    /// `Some` with UTF-8 Content-Type, otherwise `None`.
    fn content_type(headers: &http::HeaderMap) -> Option<&str> {
        headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
    }
}
