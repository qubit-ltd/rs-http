/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # HTTP Logger
//!
//! Encapsulates request and response logging behavior.
//!
//! # Author
//!
//! Haixing Hu

use crate::constants::{
    SENSITIVE_HEADER_MASK_EDGE_CHARS, SENSITIVE_HEADER_MASK_PLACEHOLDER,
    SENSITIVE_HEADER_MASK_SHORT_LEN,
};
use crate::{
    HttpClientOptions, HttpLoggingOptions, HttpRequest, HttpRequestBody, HttpResponse,
    HttpResponseMeta, SensitiveHttpHeaders,
};
use bytes::Bytes;

/// HTTP logger bound to one pair of logging options and sensitive header policy.
#[derive(Debug, Clone, Copy)]
pub struct HttpLogger<'a> {
    options: &'a HttpLoggingOptions,
    sensitive_headers: &'a SensitiveHttpHeaders,
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
            sensitive_headers: &options.sensitive_headers,
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

        let url = Self::request_log_url(request);
        tracing::trace!("--> {} {}", request.method(), url);

        let headers = request
            .effective_headers_cached()
            .unwrap_or_else(|| request.headers());

        if self.options.log_request_header {
            for (name, value) in headers {
                let value = value.to_str().unwrap_or("<non-utf8>");
                let masked = self.mask_header_value(name.as_str(), value);
                tracing::trace!("{}: {}", name.as_str(), masked);
            }
        }

        if self.options.log_request_body {
            match Self::clone_request_body_for_log(request.body()) {
                Some(bytes) => tracing::trace!("Request body: {}", self.render_body(&bytes)),
                None => tracing::trace!("Request body: <empty>"),
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
                let value = value.to_str().unwrap_or("<non-utf8>");
                let masked = self.mask_header_value(name.as_str(), value);
                tracing::trace!("{}: {}", name.as_str(), masked);
            }
        }

        if self.options.log_response_body {
            if let Some(body) = response.buffered_body_for_logging() {
                tracing::trace!("Response body: {}", self.render_body(body));
            } else if response.can_buffer_body_for_logging(self.options.body_size_limit) {
                let body = response.bytes().await?;
                tracing::trace!("Response body: {}", self.render_body(&body));
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
            response_meta.status.as_u16(),
            &response_meta.url
        );

        if self.options.log_response_header {
            for (name, value) in &response_meta.headers {
                let value = value.to_str().unwrap_or("<non-utf8>");
                let masked = self.mask_header_value(name.as_str(), value);
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
    /// Resolved URL including builder query parameters, or the raw request path
    /// when URL resolution fails before send.
    fn request_log_url(request: &HttpRequest) -> String {
        request
            .resolved_url_with_query()
            .map(|url| url.to_string())
            .unwrap_or_else(|_| request.path().to_string())
    }

    /// Returns a masked representation of a header value according to sensitivity rules.
    ///
    /// # Parameters
    /// - `name`: Header name.
    /// - `value`: Raw header value.
    ///
    /// # Returns
    /// A log-safe string when the header is sensitive; otherwise the original value.
    fn mask_header_value(&self, name: &str, value: &str) -> String {
        if value.is_empty() {
            return String::new();
        }
        if !self.sensitive_headers.contains(name) {
            return value.to_string();
        }

        let chars: Vec<char> = value.chars().collect();
        if chars.len() <= SENSITIVE_HEADER_MASK_SHORT_LEN {
            SENSITIVE_HEADER_MASK_PLACEHOLDER.to_string()
        } else {
            let edge = SENSITIVE_HEADER_MASK_EDGE_CHARS;
            let prefix: String = chars[..edge].iter().collect();
            let suffix: String = chars[chars.len() - edge..].iter().collect();
            format!("{prefix}{SENSITIVE_HEADER_MASK_PLACEHOLDER}{suffix}")
        }
    }

    /// Formats up to configured `body_size_limit` bytes of `body` for TRACE output.
    ///
    /// # Parameters
    /// - `body`: Raw bytes.
    ///
    /// # Returns
    /// Human-readable body preview string.
    fn render_body(&self, body: &Bytes) -> String {
        if body.is_empty() {
            return "<empty>".to_string();
        }

        let max_bytes = self.options.body_size_limit;
        let limit = body.len().min(max_bytes);
        let prefix = &body[..limit];
        let suffix = if body.len() > max_bytes {
            format!("...<truncated {} bytes>", body.len() - max_bytes)
        } else {
            String::new()
        };

        match std::str::from_utf8(prefix) {
            Ok(text) => format!("{}{}", text, suffix),
            Err(_) => format!("<binary {} bytes>{}", body.len(), suffix),
        }
    }

    /// Clones request body content only when body logging is needed.
    ///
    /// # Parameters
    /// - `body`: Request body variant.
    ///
    /// # Returns
    /// Optional byte payload for logger previewing.
    fn clone_request_body_for_log(body: &HttpRequestBody) -> Option<Bytes> {
        match body {
            HttpRequestBody::Bytes(bytes)
            | HttpRequestBody::Json(bytes)
            | HttpRequestBody::Form(bytes)
            | HttpRequestBody::Multipart(bytes)
            | HttpRequestBody::Ndjson(bytes) => Some(bytes.clone()),
            HttpRequestBody::Text(text) => Some(Bytes::from(text.clone())),
            HttpRequestBody::Stream(_) => None,
            HttpRequestBody::Empty => None,
        }
    }
}

/// Exercises request-log URL fallback for coverage-only tests.
///
/// # Returns
/// Raw request path returned when URL resolution fails.
#[cfg(coverage)]
#[doc(hidden)]
pub(crate) fn coverage_exercise_request_log_url_fallback() -> String {
    let client = crate::HttpClientFactory::new()
        .create_default()
        .expect("coverage HTTP client should build");
    let request = client.request(http::Method::GET, "/relative-only").build();
    HttpLogger::request_log_url(&request)
}
