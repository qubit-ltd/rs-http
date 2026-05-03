/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Reqwest/HTTP error mapping helpers used by `HttpClient` internals.

use url::Url;

use super::ReqwestErrorPhase;
use crate::{HttpError, HttpErrorKind};

/// Maps a [`reqwest::Error`] into [`HttpError`] with phase-aware timeout
/// classification and optional context.
///
/// # Parameters
/// - `error`: Underlying reqwest error.
/// - `default_kind`: Kind used when reqwest does not classify the error more
///   specifically.
/// - `phase`: Execution phase used to classify timeout errors.
/// - `method`: Optional request method to attach.
/// - `url`: Optional request URL to attach.
///
/// # Returns
/// Configured [`HttpError`] including chained source.
pub(crate) fn map_reqwest_error(
    error: reqwest::Error,
    default_kind: HttpErrorKind,
    phase: ReqwestErrorPhase,
    method: Option<http::Method>,
    url: Option<Url>,
) -> HttpError {
    let kind =
        classify_reqwest_error_kind(error.is_timeout(), error.is_decode(), phase, default_kind);

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
/// - `phase`: Phase where timeout happened.
/// - `default_kind`: Fallback kind when no specific reqwest category matches.
///
/// # Returns
/// HTTP error kind selected from reqwest metadata.
fn classify_reqwest_error_kind(
    is_timeout: bool,
    is_decode: bool,
    phase: ReqwestErrorPhase,
    default_kind: HttpErrorKind,
) -> HttpErrorKind {
    if is_timeout {
        classify_timeout_kind(phase)
    } else if is_decode {
        HttpErrorKind::Decode
    } else {
        default_kind
    }
}

/// Classifies timeout errors from known phase metadata.
///
/// # Parameters
/// - `phase`: Phase where timeout happened.
///
/// # Returns
/// Timeout kind inferred from the phase.
fn classify_timeout_kind(phase: ReqwestErrorPhase) -> HttpErrorKind {
    match phase {
        ReqwestErrorPhase::Send => HttpErrorKind::RequestTimeout,
        ReqwestErrorPhase::Read => HttpErrorKind::ReadTimeout,
    }
}
