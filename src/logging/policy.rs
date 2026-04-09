/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # Logging Policy Helpers
//!
//! Encapsulates request and response logging behavior.
//!
//! # Author
//!
//! Haixing Hu

use bytes::Bytes;
use http::{HeaderMap, Method, StatusCode};
use url::Url;

use crate::logging::mask_header_value;
use crate::{HttpLoggingOptions, SensitiveHeaders};

/// Emits TRACE logs for an outbound request when logging is enabled and the TRACE level is active.
///
/// # Parameters
/// - `method`: HTTP method.
/// - `url`: Full request URL.
/// - `headers`: Outgoing headers (values may be masked).
/// - `body`: Optional body preview source.
/// - `options`: What to log and body truncation limit.
/// - `sensitive_headers`: Header names to mask.
///
/// # Returns
/// Nothing; no-op when disabled or TRACE off.
pub fn log_request(
    method: &Method,
    url: &Url,
    headers: &HeaderMap,
    body: Option<&Bytes>,
    options: &HttpLoggingOptions,
    sensitive_headers: &SensitiveHeaders,
) {
    if !options.enabled || !tracing::enabled!(tracing::Level::TRACE) {
        return;
    }

    tracing::trace!("--> {} {}", method, url);

    if options.log_request_header {
        for (name, value) in headers {
            let value = value.to_str().unwrap_or("<non-utf8>");
            let masked = mask_header_value(name.as_str(), value, sensitive_headers);
            tracing::trace!("{}: {}", name.as_str(), masked);
        }
    }

    if options.log_request_body {
        match body {
            Some(bytes) => {
                let body = render_body(bytes, options.body_size_limit);
                tracing::trace!("Request body: {}", body);
            }
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
/// - `options`: Logging toggles and size limit.
/// - `sensitive_headers`: Names to mask.
///
/// # Returns
/// Nothing; no-op when disabled or TRACE off.
pub fn log_response(
    status: StatusCode,
    url: &Url,
    headers: &HeaderMap,
    body: &Bytes,
    options: &HttpLoggingOptions,
    sensitive_headers: &SensitiveHeaders,
) {
    if !options.enabled || !tracing::enabled!(tracing::Level::TRACE) {
        return;
    }

    tracing::trace!("<-- {} {}", status.as_u16(), url);

    if options.log_response_header {
        for (name, value) in headers {
            let value = value.to_str().unwrap_or("<non-utf8>");
            let masked = mask_header_value(name.as_str(), value, sensitive_headers);
            tracing::trace!("{}: {}", name.as_str(), masked);
        }
    }

    if options.log_response_body {
        let rendered = render_body(body, options.body_size_limit);
        tracing::trace!("Response body: {}", rendered);
    }
}

/// Logs response line and headers for a streaming call without reading the body stream.
///
/// # Parameters
/// - `status`: Response status.
/// - `url`: Response URL.
/// - `headers`: Response headers.
/// - `options`: Logging toggles.
/// - `sensitive_headers`: Names to mask.
///
/// # Returns
/// Nothing; no-op when disabled or TRACE off.
pub fn log_stream_response_headers(
    status: StatusCode,
    url: &Url,
    headers: &HeaderMap,
    options: &HttpLoggingOptions,
    sensitive_headers: &SensitiveHeaders,
) {
    if !options.enabled || !tracing::enabled!(tracing::Level::TRACE) {
        return;
    }

    tracing::trace!("<-- {} {} (stream)", status.as_u16(), url);

    if options.log_response_header {
        for (name, value) in headers {
            let value = value.to_str().unwrap_or("<non-utf8>");
            let masked = mask_header_value(name.as_str(), value, sensitive_headers);
            tracing::trace!("{}: {}", name.as_str(), masked);
        }
    }
}

/// Formats up to `max_bytes` of `body` for TRACE output, marking truncation and binary data.
///
/// # Parameters
/// - `body`: Raw bytes.
/// - `max_bytes`: Maximum prefix length to stringify.
///
/// # Returns
/// Human-readable string for logs.
fn render_body(body: &Bytes, max_bytes: usize) -> String {
    if body.is_empty() {
        return "<empty>".to_string();
    }

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
