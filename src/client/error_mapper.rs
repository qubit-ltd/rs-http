/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Reqwest/HTTP error mapping helpers used by `HttpClient` internals.

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
