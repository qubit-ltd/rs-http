// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # HTTP Logger
//!
//! Encapsulates request and response logging behavior.

use http::header::CONTENT_TYPE;
use qubit_redact::http::HttpRedactor;
use tracing::callsite::DefaultCallsite;
use tracing::metadata::Kind;
use tracing::Metadata;

use crate::redact::RedactedLogger;
use crate::{
    HttpClientOptions,
    HttpLoggingOptions,
    HttpRequest,
    HttpRequestBody,
    HttpResponse,
    HttpResponseMeta,
};

const UNRESOLVED_REQUEST_URL: &str = "<unresolved request URL>";
const STREAMING_REQUEST_BODY_SKIPPED: &str =
    "<skipped: streaming request body>";

/// Callsite metadata used to query the current dispatcher directly, bypassing
/// shared interest caching that can change while thread-local subscribers are
/// installed concurrently.
static HTTP_LOGGER_ENABLED_CALLSITE: DefaultCallsite =
    DefaultCallsite::new(&HTTP_LOGGER_ENABLED_METADATA);
static HTTP_LOGGER_ENABLED_METADATA: Metadata<'static> = tracing::metadata! {
    name: "qubit_http_logger_enabled",
    target: module_path!(),
    level: tracing::Level::TRACE,
    fields: &[],
    callsite: &HTTP_LOGGER_ENABLED_CALLSITE,
    kind: Kind::EVENT,
};

/// HTTP logger bound to one pair of logging options and a redactor policy.
#[derive(Debug, Clone)]
pub struct HttpLogger<'a> {
    options: &'a HttpLoggingOptions,
    redacted_logger: RedactedLogger,
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
        let log_redactor =
            HttpRedactor::new(options.log_redaction_policy.clone());
        Self::from_options_with_redactor(options, log_redactor)
    }

    /// Creates a logger bound to the caller's shared redactor snapshot.
    ///
    /// # Parameters
    ///
    /// * `options` - Client options carrying logging switches.
    /// * `log_redactor` - Exact redactor propagated by the owning client.
    ///
    /// # Returns
    ///
    /// A logger that shares the caller's immutable redactor snapshot.
    pub(crate) fn from_options_with_redactor(
        options: &'a HttpClientOptions,
        log_redactor: HttpRedactor,
    ) -> Self {
        Self {
            options: &options.logging,
            redacted_logger: RedactedLogger::from_options_with_redactor(
                options,
                log_redactor,
            ),
        }
    }

    /// Emits TRACE logs for an outbound request when logging is enabled and
    /// TRACE is active.
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
            tracing::trace!("{}", self.redacted_logger.headers(headers));
        }

        if self.options.log_request_body {
            match Self::request_body_for_log(request) {
                RequestBodyLogPreview::Bytes(bytes) => {
                    let content_type = Self::content_type(headers);
                    tracing::trace!(
                        "Request body: {}",
                        self.redacted_logger.body(bytes, content_type)
                    );
                }
                RequestBodyLogPreview::Empty => {
                    tracing::trace!("Request body: <empty>")
                }
                RequestBodyLogPreview::Skipped(reason) => {
                    tracing::trace!("Request body: {reason}")
                }
            }
        }
    }

    /// Emits TRACE logs for a completed response (headers and optional body
    /// preview).
    ///
    /// # Parameters
    /// - `response`: Response object (status/url/headers/body cache).
    ///
    /// # Returns
    /// `Ok(())` on success; no-op when disabled or TRACE off.
    ///
    /// # Errors
    /// Returns [`crate::HttpError`] when reading the response body for logging
    /// fails.
    pub async fn log_response(
        &self,
        response: &mut HttpResponse,
    ) -> crate::HttpResult<()> {
        if !self.is_trace_enabled() {
            return Ok(());
        }

        tracing::trace!(
            "<-- {} {}",
            response.status().as_u16(),
            self.redacted_logger.url(response.url())
        );

        if self.options.log_response_header {
            tracing::trace!(
                "{}",
                self.redacted_logger.headers(response.headers())
            );
        }

        if self.options.log_response_body {
            let content_type = Self::content_type(response.headers()).cloned();
            if let Some(body) = response.buffered_body_for_logging() {
                tracing::trace!(
                    "Response body: {}",
                    self.redacted_logger
                        .body(body.as_ref(), content_type.as_ref())
                );
            } else if response
                .can_buffer_body_for_logging(self.options.body_size_limit)
            {
                let body = response.bytes().await?;
                tracing::trace!(
                    "Response body: {}",
                    self.redacted_logger
                        .body(body.as_ref(), content_type.as_ref())
                );
            } else {
                tracing::trace!(
                    "Response body: <skipped: streaming or unknown-size body>"
                );
            }
        }
        Ok(())
    }

    /// Logs response line and headers for a streaming call without reading the
    /// body stream.
    ///
    /// # Parameters
    /// - `response_meta`: Response metadata (status/url/headers).
    ///
    /// # Returns
    /// Nothing; no-op when disabled or TRACE off.
    pub fn log_stream_response_headers(
        &self,
        response_meta: &HttpResponseMeta,
    ) {
        if !self.is_trace_enabled() {
            return;
        }

        tracing::trace!(
            "<-- {} {} (stream)",
            response_meta.status().as_u16(),
            self.redacted_logger.url(response_meta.url())
        );

        if self.options.log_response_header {
            tracing::trace!(
                "{}",
                self.redacted_logger.headers(response_meta.headers())
            );
        }
    }

    /// Returns whether TRACE logs should be emitted under current options and
    /// subscriber state.
    ///
    /// # Returns
    /// `true` when logging is enabled and TRACE is active.
    pub fn is_trace_enabled(&self) -> bool {
        self.options.enabled
            && tracing::dispatcher::get_default(|dispatcher| {
                dispatcher.enabled(&HTTP_LOGGER_ENABLED_METADATA)
            })
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
            .map(|url| self.redacted_logger.url(&url))
            .map(|url| url.into_owned())
            .unwrap_or_else(|_| UNRESOLVED_REQUEST_URL.to_string())
    }

    /// Borrows request body content only when body logging is safe.
    ///
    /// # Parameters
    /// - `request`: Prepared request snapshot.
    ///
    /// # Returns
    /// Body preview category for logger rendering.
    fn request_body_for_log(
        request: &HttpRequest,
    ) -> RequestBodyLogPreview<'_> {
        if request.has_streaming_body() {
            return RequestBodyLogPreview::Skipped(
                STREAMING_REQUEST_BODY_SKIPPED,
            );
        }
        match request.body() {
            HttpRequestBody::Bytes(bytes)
            | HttpRequestBody::Json(bytes)
            | HttpRequestBody::Form(bytes)
            | HttpRequestBody::Multipart(bytes)
            | HttpRequestBody::Ndjson(bytes) => {
                RequestBodyLogPreview::Bytes(bytes.as_ref())
            }
            HttpRequestBody::Text(text) => {
                RequestBodyLogPreview::Bytes(text.as_bytes())
            }
            HttpRequestBody::Stream(_) => {
                RequestBodyLogPreview::Skipped(STREAMING_REQUEST_BODY_SKIPPED)
            }
            HttpRequestBody::Empty => RequestBodyLogPreview::Empty,
        }
    }

    /// Extracts a Content-Type header value from a header map.
    ///
    /// # Parameters
    /// - `headers`: Headers to inspect.
    ///
    /// # Returns
    /// `Some` when Content-Type is present, including when its value is not
    /// valid UTF-8; otherwise `None`.
    fn content_type(headers: &http::HeaderMap) -> Option<&http::HeaderValue> {
        headers.get(CONTENT_TYPE)
    }
}
