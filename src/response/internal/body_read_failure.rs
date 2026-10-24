// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Snapshot the diagnostic context needed to report repeated body-read
//! failures.

use std::sync::Arc;
use std::sync::Mutex;

use http::Method;
use http::StatusCode;
use qubit_redact::Redactor;
use url::Url;

use crate::HttpError;
use crate::HttpErrorKind;

/// Cloneable diagnostic context retained after a backend body has been
/// consumed.
///
/// Keeping this snapshot lets later reads report the original failure category,
/// message, request context, and redaction policy without retaining the backend
/// error or body stream.
#[derive(Debug, Clone)]
pub(super) struct BodyReadFailure {
    /// Classification used by callers to handle or retry the original failure.
    kind: HttpErrorKind,
    /// Human-readable message copied from the original failure.
    message: String,
    /// Request method associated with the failure, when available.
    method: Option<Method>,
    /// Request URL associated with the failure, when available.
    url: Option<Url>,
    /// Response status associated with the failure, when available.
    status: Option<StatusCode>,
    /// Redaction policy used by the originating response to render diagnostics.
    log_redactor: Redactor,
}

impl BodyReadFailure {
    /// Copies the diagnostic context needed to reproduce a later read error.
    ///
    /// # Parameters
    /// - `error`: Original body-read error whose category, message, request
    ///   context, and redactor are retained. The source chain and unrelated
    ///   response metadata are intentionally not copied.
    ///
    /// # Returns
    /// Snapshot used to construct consistent errors after the body is consumed.
    pub(super) fn from_error(error: &HttpError) -> Self {
        Self {
            kind: error.kind,
            message: error.message.clone(),
            method: error.method.clone(),
            url: error.url.clone(),
            status: error.status,
            log_redactor: error.log_redactor.clone(),
        }
    }

    /// Reconstructs an [`HttpError`] describing the earlier body-read failure.
    ///
    /// The original context and redactor are restored, while the message is
    /// prefixed to identify this error as a failure from a previous read.
    ///
    /// # Returns
    /// Error with the original category, request context, and redaction policy.
    pub(super) fn to_error(&self) -> HttpError {
        let mut error = HttpError::new(
            self.kind,
            format!("previous response body read failed: {}", self.message),
        );
        if let Some(method) = self.method.as_ref() {
            error = error.with_method(method);
        }
        if let Some(url) = self.url.as_ref() {
            error = error.with_url(url);
        }
        if let Some(status) = self.status {
            error = error.with_status(status);
        }
        error.with_log_redactor(self.log_redactor.clone())
    }
}

/// Thread-safe slot for retaining the first body-read failure on a response.
///
/// The mutex serializes updates and later reads across cloned response handles.
pub(super) type BodyReadFailureState = Arc<Mutex<Option<BodyReadFailure>>>;
