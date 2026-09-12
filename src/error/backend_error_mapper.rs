// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Reqwest/HTTP error mapping helpers used by `HttpClient` internals.

use std::error::Error;

use url::Url;

use super::ReqwestErrorPhase;
use crate::HttpError;
use crate::HttpErrorKind;
use crate::client::RedirectOriginViolation;

/// Maps a [`reqwest::Error`] into [`HttpError`] with phase-aware timeout
/// classification and optional context.
///
/// # Parameters
/// - `error`: Underlying reqwest error.
/// - `default_kind`: Kind used when reqwest does not classify the error more
///   specifically.
/// - `phase`: Execution phase used to classify timeout errors.
/// - `method`: Request method to attach.
/// - `url`: Request URL to attach.
///
/// # Returns
/// Configured [`HttpError`] including chained source.
pub(crate) fn map_reqwest_error(
    error: reqwest::Error,
    default_kind: HttpErrorKind,
    phase: ReqwestErrorPhase,
    method: http::Method,
    url: Url,
) -> HttpError {
    let is_timeout = error.is_timeout();
    let error = error.without_url();
    let kind = if contains_redirect_origin_violation(&error) {
        HttpErrorKind::OriginPolicy
    } else {
        classify_reqwest_error_kind(is_timeout, phase, default_kind)
    };

    HttpError::new(kind, "HTTP transport request failed")
        .with_method(&method)
        .with_url(&url)
        .with_source(error)
}

fn contains_redirect_origin_violation(error: &reqwest::Error) -> bool {
    let mut source = error.source();
    while let Some(candidate) = source {
        if candidate.downcast_ref::<RedirectOriginViolation>().is_some() {
            return true;
        }
        source = candidate.source();
    }
    false
}

/// Classifies reqwest errors from extracted metadata.
///
/// # Parameters
/// - `is_timeout`: Whether reqwest marked the error as timeout.
/// - `phase`: Phase where timeout happened.
/// - `default_kind`: Fallback kind when no specific reqwest category matches.
///
/// # Returns
/// HTTP error kind selected from reqwest metadata.
fn classify_reqwest_error_kind(
    is_timeout: bool,
    phase: ReqwestErrorPhase,
    default_kind: HttpErrorKind,
) -> HttpErrorKind {
    if is_timeout {
        classify_timeout_kind(phase)
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
