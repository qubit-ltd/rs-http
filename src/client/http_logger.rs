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
use qubit_redact::Redactor;
use tracing::Metadata;
use tracing::callsite::DefaultCallsite;
use tracing::metadata::Kind;

use crate::HttpClientOptions;
use crate::HttpLoggingOptions;
use crate::HttpRequest;
use crate::HttpRequestBody;
use crate::HttpResponse;
use crate::HttpResponseMeta;
use crate::redact::BodyPreview;
use crate::redact::RedactedLogger;

const UNRESOLVED_REQUEST_URL: &str = "<unresolved request URL>";
const STREAMING_REQUEST_BODY_SKIPPED: &str = "<skipped: streaming request body>";

/// Callsite metadata used to query the current dispatcher directly, bypassing
/// shared interest caching that can change while thread-local subscribers are
/// installed concurrently.
static HTTP_LOGGER_ENABLED_CALLSITE: DefaultCallsite = DefaultCallsite::new(&HTTP_LOGGER_ENABLED_METADATA);
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
        let log_redactor = Redactor::new(options.log_redaction_policy.clone());
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
    pub(crate) fn from_options_with_redactor(options: &'a HttpClientOptions, log_redactor: Redactor) -> Self {
        Self {
            options: &options.logging,
            redacted_logger: RedactedLogger::from_options_with_redactor(options, log_redactor),
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

        let headers = request.effective_headers_cached().unwrap_or_else(|| request.headers());
        let mut batch = self.redacted_logger.redactor().batch();
        let url_handle = request
            .resolved_url()
            .ok()
            .map(|url| batch.redact_http_url(url.as_str()));
        let header_handle = self
            .options
            .log_request_header
            .then(|| batch.redact_http_headers(headers));
        let body_preview = self
            .options
            .log_request_body
            .then(|| Self::request_body_for_log(request));
        let body_handle = match body_preview.as_ref() {
            Some(RequestBodyLogPreview::Bytes(bytes)) => Some(batch.redact_http_body(
                BodyPreview::new(bytes, self.options.body_size_limit).capture(),
                Self::content_type(headers),
            )),
            _ => None,
        };
        let diagnostics = batch.finish_for_diagnostics("<redaction incomplete>");
        let url = url_handle
            .map(|handle| diagnostics.text(handle).as_str().to_owned())
            .unwrap_or_else(|| UNRESOLVED_REQUEST_URL.to_owned());
        tracing::trace!("--> {} {}", request.method(), url);
        if let Some(handle) = header_handle {
            tracing::trace!("{}", diagnostics.text(handle));
        }
        if let Some(preview) = body_preview {
            match (preview, body_handle) {
                (RequestBodyLogPreview::Bytes(_), Some(handle)) => {
                    tracing::trace!("Request body: {}", diagnostics.text(handle));
                }
                (RequestBodyLogPreview::Empty, _) => tracing::trace!("Request body: <empty>"),
                (RequestBodyLogPreview::Skipped(reason), _) => tracing::trace!("Request body: {reason}"),
                (RequestBodyLogPreview::Bytes(_), None) => {
                    tracing::trace!("Request body: <redaction incomplete>");
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
    pub async fn log_response(&self, response: &mut HttpResponse) -> crate::HttpResult<()> {
        if !self.is_trace_enabled() {
            return Ok(());
        }

        let mut batch = self.redacted_logger.redactor().batch();
        let url_handle = batch.redact_http_url(response.url().as_str());
        let header_handle = self
            .options
            .log_response_header
            .then(|| batch.redact_http_headers(response.headers()));
        if self.options.log_response_body && !batch.is_output_exhausted() {
            let content_type = Self::content_type(response.headers()).cloned();
            let mut body_empty = false;
            let body_handle = if let Some(body) = response.buffered_body_for_logging() {
                if body.is_empty() {
                    body_empty = true;
                    None
                } else {
                    Some(batch.redact_http_body(
                        BodyPreview::new(body.as_ref(), self.options.body_size_limit).capture(),
                        content_type.as_ref(),
                    ))
                }
            } else if response.can_buffer_body_for_logging(self.options.body_size_limit) {
                let body = match response.bytes().await {
                    Ok(body) => body,
                    Err(error) => {
                        let diagnostics = batch.finish_for_diagnostics("<redaction incomplete>");
                        tracing::trace!("<-- {} {}", response.status().as_u16(), diagnostics.text(url_handle));
                        if let Some(handle) = header_handle {
                            tracing::trace!("{}", diagnostics.text(handle));
                        }
                        return Err(error);
                    }
                };
                if body.is_empty() {
                    body_empty = true;
                    None
                } else {
                    Some(batch.redact_http_body(
                        BodyPreview::new(body.as_ref(), self.options.body_size_limit).capture(),
                        content_type.as_ref(),
                    ))
                }
            } else {
                None
            };
            let diagnostics = batch.finish_for_diagnostics("<redaction incomplete>");
            tracing::trace!("<-- {} {}", response.status().as_u16(), diagnostics.text(url_handle));
            if let Some(handle) = header_handle {
                tracing::trace!("{}", diagnostics.text(handle));
            }
            if body_empty {
                tracing::trace!("Response body: <empty>");
            } else if let Some(handle) = body_handle {
                tracing::trace!("Response body: {}", diagnostics.text(handle));
            } else {
                tracing::trace!("Response body: <skipped: streaming or unknown-size body>");
            }
        } else {
            let diagnostics = batch.finish_for_diagnostics("<redaction incomplete>");
            tracing::trace!("<-- {} {}", response.status().as_u16(), diagnostics.text(url_handle));
            if let Some(handle) = header_handle {
                tracing::trace!("{}", diagnostics.text(handle));
            }
            if self.options.log_response_body {
                tracing::trace!("Response body: <redaction incomplete>");
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
    pub fn log_stream_response_headers(&self, response_meta: &HttpResponseMeta) {
        if !self.is_trace_enabled() {
            return;
        }

        let mut batch = self.redacted_logger.redactor().batch();
        let url_handle = batch.redact_http_url(response_meta.url().as_str());
        let header_handle = self
            .options
            .log_response_header
            .then(|| batch.redact_http_headers(response_meta.headers()));
        let diagnostics = batch.finish_for_diagnostics("<redaction incomplete>");
        tracing::trace!(
            "<-- {} {} (stream)",
            response_meta.status().as_u16(),
            diagnostics.text(url_handle)
        );
        if let Some(handle) = header_handle {
            tracing::trace!("{}", diagnostics.text(handle));
        }
    }

    /// Returns whether TRACE logs should be emitted under current options and
    /// subscriber state.
    ///
    /// # Returns
    /// `true` when logging is enabled and TRACE is active.
    pub fn is_trace_enabled(&self) -> bool {
        self.options.enabled
            && tracing::dispatcher::get_default(|dispatcher| dispatcher.enabled(&HTTP_LOGGER_ENABLED_METADATA))
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
            HttpRequestBody::Stream(_) => RequestBodyLogPreview::Skipped(STREAMING_REQUEST_BODY_SKIPPED),
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
