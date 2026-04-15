/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Reqwest/HTTP error mapping helpers used by `HttpClient` internals.

use std::time::{Duration, SystemTime};

use http::header::RETRY_AFTER;
use http::{HeaderMap, StatusCode};
use httpdate::parse_http_date;
use url::Url;

use crate::{HttpError, HttpErrorKind};

/// Maps a [`reqwest::Error`] into [`HttpError`] with best-effort
/// [`HttpErrorKind`] and optional context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReqwestErrorPhase {
    /// Error happened while sending request / waiting first response bytes.
    Send,
    /// Error happened while reading response body.
    Read,
}

/// Maps a [`reqwest::Error`] into [`HttpError`] with phase-aware timeout
/// classification and optional context.
///
/// # Parameters
/// - `error`: Underlying reqwest error.
/// - `default_kind`: Kind used when reqwest does not classify the error more
///   specifically.
/// - `phase`: Optional execution phase used to classify timeout errors.
/// - `method`: Optional request method to attach.
/// - `url`: Optional request URL to attach.
///
/// # Returns
/// Configured [`HttpError`] including chained source.
pub(crate) fn map_reqwest_error(
    error: reqwest::Error,
    default_kind: HttpErrorKind,
    phase: Option<ReqwestErrorPhase>,
    method: Option<http::Method>,
    url: Option<Url>,
) -> HttpError {
    let kind = if error.is_timeout() {
        classify_reqwest_timeout_kind(&error, phase)
    } else if error.is_decode() {
        HttpErrorKind::Decode
    } else if error.is_status() {
        HttpErrorKind::Status
    } else if error.is_request() && error.url().is_none() {
        HttpErrorKind::InvalidUrl
    } else {
        default_kind
    };

    let mut result = HttpError::new(kind, format!("HTTP transport error: {}", error));
    if let Some(method) = method {
        result = result.with_method(&method);
    }
    if let Some(url) = url {
        result = result.with_url(&url);
    }
    result.with_source(error)
}

/// Classifies reqwest timeout errors by execution phase.
///
/// # Parameters
/// - `error`: Reqwest timeout error to classify.
/// - `phase`: Optional phase where timeout happened.
///
/// # Returns
/// Timeout kind inferred from phase:
/// - send phase: `ConnectTimeout` when reqwest marks connect errors; otherwise `RequestTimeout`;
/// - read phase: `ReadTimeout`;
/// - unknown phase: `RequestTimeout`.
fn classify_reqwest_timeout_kind(
    error: &reqwest::Error,
    phase: Option<ReqwestErrorPhase>,
) -> HttpErrorKind {
    match phase {
        Some(ReqwestErrorPhase::Send) => {
            if error.is_connect() {
                HttpErrorKind::ConnectTimeout
            } else {
                HttpErrorKind::RequestTimeout
            }
        }
        Some(ReqwestErrorPhase::Read) => HttpErrorKind::ReadTimeout,
        None => HttpErrorKind::RequestTimeout,
    }
}

/// Parses `Retry-After` from response headers when status is retryable.
///
/// # Parameters
/// - `status`: HTTP status code.
/// - `headers`: Response headers.
///
/// # Returns
/// Parsed retry delay when `status` is `429` or `5xx` and `Retry-After` is
/// present in `delta-seconds` or HTTP-date format.
pub(super) fn parse_retry_after(status: StatusCode, headers: &HeaderMap) -> Option<Duration> {
    if !is_retry_after_applicable_status(status) {
        return None;
    }
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_retry_after_value)
}

/// Returns whether a status code should honor `Retry-After`.
///
/// # Parameters
/// - `status`: HTTP status code.
///
/// # Returns
/// `true` for `429` and `5xx` statuses; otherwise `false`.
fn is_retry_after_applicable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// Parses a `Retry-After` header value as delta-seconds or HTTP-date.
///
/// # Parameters
/// - `value`: Raw `Retry-After` header value.
///
/// # Returns
/// Parsed duration, or `None` when value is neither valid delta-seconds nor a
/// valid HTTP-date.
fn parse_retry_after_value(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(seconds) = trimmed.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let retry_at = parse_http_date(trimmed).ok()?;
    let now = SystemTime::now();
    Some(
        retry_at
            .duration_since(now)
            .unwrap_or_else(|_| Duration::from_secs(0)),
    )
}

/// Renders a human-readable error-body preview from raw bytes.
///
/// # Parameters
/// - `bytes`: Captured body bytes (already size-limited).
/// - `truncated`: Whether additional bytes were omitted.
///
/// # Returns
/// UTF-8 text preview or binary placeholder with truncation suffix when needed.
pub(super) fn render_error_body_preview(bytes: &[u8], truncated: bool) -> String {
    if bytes.is_empty() {
        return "<empty>".to_string();
    }

    let suffix = if truncated { "...<truncated>" } else { "" };
    match std::str::from_utf8(bytes) {
        Ok(text) => format!("{text}{suffix}"),
        Err(_) => format!("<binary {} bytes>{suffix}", bytes.len()),
    }
}
