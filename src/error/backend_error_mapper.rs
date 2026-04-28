/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Reqwest/HTTP error mapping helpers used by `HttpClient` internals.

use parse_display::{Display, FromStr as DeriveFromStr};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{HttpError, HttpErrorKind};

/// Maps a [`reqwest::Error`] into [`HttpError`] with best-effort
/// [`HttpErrorKind`] and optional context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, DeriveFromStr)]
#[serde(rename_all = "snake_case")]
#[display(style = "snake_case")]
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
    let kind = classify_reqwest_error_kind(
        error.is_timeout(),
        error.is_decode(),
        error.is_status(),
        error.is_request() && error.url().is_none(),
        error.is_connect(),
        phase,
        default_kind,
    );

    let mut result = HttpError::new(kind, format!("HTTP transport error: {}", error));
    if let Some(method) = method {
        result = result.with_method(&method);
    }
    if let Some(url) = url {
        result = result.with_url(&url);
    }
    result.with_source(error)
}

/// Classifies reqwest errors from extracted metadata.
///
/// # Parameters
/// - `is_timeout`: Whether reqwest marked the error as timeout.
/// - `is_decode`: Whether reqwest marked the error as decode failure.
/// - `is_status`: Whether reqwest marked the error as HTTP status failure.
/// - `is_request_without_url`: Whether reqwest marked the error as request
///   construction failure without a parsed URL.
/// - `is_connect`: Whether reqwest marked the error as connect failure.
/// - `phase`: Optional phase where timeout happened.
/// - `default_kind`: Fallback kind when no specific reqwest category matches.
///
/// # Returns
/// HTTP error kind selected from reqwest metadata.
fn classify_reqwest_error_kind(
    is_timeout: bool,
    is_decode: bool,
    is_status: bool,
    is_request_without_url: bool,
    is_connect: bool,
    phase: Option<ReqwestErrorPhase>,
    default_kind: HttpErrorKind,
) -> HttpErrorKind {
    if is_timeout {
        classify_timeout_kind(is_connect, phase)
    } else if is_decode {
        HttpErrorKind::Decode
    } else if is_status {
        HttpErrorKind::Status
    } else if is_request_without_url {
        HttpErrorKind::InvalidUrl
    } else {
        default_kind
    }
}

/// Classifies timeout errors from known phase and connect metadata.
///
/// # Parameters
/// - `is_connect`: Whether reqwest marked the timeout as a connect failure.
/// - `phase`: Optional phase where timeout happened.
///
/// # Returns
/// Timeout kind inferred from the phase and connect flag.
fn classify_timeout_kind(is_connect: bool, phase: Option<ReqwestErrorPhase>) -> HttpErrorKind {
    match phase {
        Some(ReqwestErrorPhase::Send) => {
            if is_connect {
                HttpErrorKind::ConnectTimeout
            } else {
                HttpErrorKind::RequestTimeout
            }
        }
        Some(ReqwestErrorPhase::Read) => HttpErrorKind::ReadTimeout,
        None => HttpErrorKind::RequestTimeout,
    }
}

/// Classifies synthetic timeout metadata for coverage-only tests.
///
/// # Parameters
/// - `is_connect`: Whether the timeout should be treated as a connect timeout.
/// - `phase`: Optional synthetic request phase.
///
/// # Returns
/// Timeout kind inferred by the same helper used for reqwest errors.
#[cfg(coverage)]
#[doc(hidden)]
pub(crate) fn coverage_classify_timeout_kind(
    is_connect: bool,
    phase: Option<ReqwestErrorPhase>,
) -> HttpErrorKind {
    classify_timeout_kind(is_connect, phase)
}

/// Classifies synthetic reqwest metadata for coverage-only tests.
///
/// # Parameters
/// - `is_timeout`: Whether the error should be treated as timeout.
/// - `is_decode`: Whether the error should be treated as decode failure.
/// - `is_status`: Whether the error should be treated as status failure.
/// - `is_request_without_url`: Whether the error should be treated as invalid
///   URL / request construction failure.
/// - `is_connect`: Whether the timeout should be treated as connect timeout.
/// - `phase`: Optional synthetic request phase.
/// - `default_kind`: Fallback kind when no synthetic category matches.
///
/// # Returns
/// HTTP error kind inferred by the same helper used for reqwest errors.
#[cfg(coverage)]
#[doc(hidden)]
pub(crate) fn coverage_classify_reqwest_error_kind(
    is_timeout: bool,
    is_decode: bool,
    is_status: bool,
    is_request_without_url: bool,
    is_connect: bool,
    phase: Option<ReqwestErrorPhase>,
    default_kind: HttpErrorKind,
) -> HttpErrorKind {
    classify_reqwest_error_kind(
        is_timeout,
        is_decode,
        is_status,
        is_request_without_url,
        is_connect,
        phase,
        default_kind,
    )
}
