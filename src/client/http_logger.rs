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

use bytes::Bytes;
use http::{HeaderMap, Method, StatusCode};
use url::Url;

use crate::constants::{
    SENSITIVE_HEADER_MASK_EDGE_CHARS, SENSITIVE_HEADER_MASK_PLACEHOLDER,
    SENSITIVE_HEADER_MASK_SHORT_LEN,
};
use crate::{HttpClientOptions, HttpLoggingOptions, SensitiveHeaders};

/// HTTP logger bound to one pair of logging options and sensitive header policy.
#[derive(Debug, Clone, Copy)]
pub struct HttpLogger<'a> {
    options: &'a HttpLoggingOptions,
    sensitive_headers: &'a SensitiveHeaders,
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
    /// - `method`: HTTP method.
    /// - `url`: Full request URL.
    /// - `headers`: Outgoing headers (values may be masked).
    /// - `body`: Optional body preview source.
    ///
    /// # Returns
    /// Nothing; no-op when disabled or TRACE off.
    pub fn log_request(
        &self,
        method: &Method,
        url: &Url,
        headers: &HeaderMap,
        body: Option<&Bytes>,
    ) {
        if !self.is_trace_enabled() {
            return;
        }

        tracing::trace!("--> {} {}", method, url);

        if self.options.log_request_header {
            for (name, value) in headers {
                let value = value.to_str().unwrap_or("<non-utf8>");
                let masked = self.mask_header_value(name.as_str(), value);
                tracing::trace!("{}: {}", name.as_str(), masked);
            }
        }

        if self.options.log_request_body {
            match body {
                Some(bytes) => tracing::trace!("Request body: {}", self.render_body(bytes)),
                None => tracing::trace!("Request body: <empty>"),
            }
        }
    }

    /// Emits TRACE logs for a completed response (headers and optional body preview).
    ///
    /// # Parameters
    /// - `status`: Response status.
    /// - `url`: Response URL.
    /// - `headers`: Response headers (masked per policy).
    /// - `body`: Full body bytes for optional preview.
    ///
    /// # Returns
    /// Nothing; no-op when disabled or TRACE off.
    pub fn log_response(&self, status: StatusCode, url: &Url, headers: &HeaderMap, body: &Bytes) {
        if !self.is_trace_enabled() {
            return;
        }

        tracing::trace!("<-- {} {}", status.as_u16(), url);

        if self.options.log_response_header {
            for (name, value) in headers {
                let value = value.to_str().unwrap_or("<non-utf8>");
                let masked = self.mask_header_value(name.as_str(), value);
                tracing::trace!("{}: {}", name.as_str(), masked);
            }
        }

        if self.options.log_response_body {
            tracing::trace!("Response body: {}", self.render_body(body));
        }
    }

    /// Logs response line and headers for a streaming call without reading the body stream.
    ///
    /// # Parameters
    /// - `status`: Response status.
    /// - `url`: Response URL.
    /// - `headers`: Response headers.
    ///
    /// # Returns
    /// Nothing; no-op when disabled or TRACE off.
    pub fn log_stream_response_headers(&self, status: StatusCode, url: &Url, headers: &HeaderMap) {
        if !self.is_trace_enabled() {
            return;
        }

        tracing::trace!("<-- {} {} (stream)", status.as_u16(), url);

        if self.options.log_response_header {
            for (name, value) in headers {
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

    /// Returns whether [`Self::log_request`] will read and emit a request body preview.
    ///
    /// Callers can use this to avoid cloning request body bytes when TRACE logging will not use them.
    ///
    /// # Returns
    /// `true` when TRACE logging is active and request body logging is enabled.
    pub fn should_log_request_body(&self) -> bool {
        self.is_trace_enabled() && self.options.log_request_body
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
}
