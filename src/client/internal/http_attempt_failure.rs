// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Failure metadata captured from one intercepted HTTP attempt.

use std::error::Error;
use std::fmt;

use crate::HttpError;

/// HTTP error plus retry decisions derived from the final attempt request.
#[derive(Debug)]
pub(crate) struct HttpAttemptFailure {
    /// Public HTTP error produced by the attempt.
    pub(crate) error: HttpError,
    /// Whether the final request permits another attempt.
    pub(in crate::client) retry_allowed: bool,
    /// Whether the final request permits a Retry-After hint.
    pub(crate) honor_retry_after: bool,
}

impl HttpAttemptFailure {
    /// Creates a failure with the final request's retry decisions.
    ///
    /// # Parameters
    /// - `error`: HTTP error produced by the attempt.
    /// - `retry_allowed`: Whether the request may be retried.
    /// - `honor_retry_after`: Whether a Retry-After hint may be used.
    ///
    /// # Returns
    /// Failure metadata for the completed attempt.
    pub(in crate::client) fn new(error: HttpError, retry_allowed: bool, honor_retry_after: bool) -> Self {
        Self {
            error,
            retry_allowed,
            honor_retry_after,
        }
    }

    /// Consumes this wrapper and returns its HTTP error.
    ///
    /// # Returns
    /// The underlying HTTP error.
    pub(in crate::client) fn into_error(self) -> HttpError {
        self.error
    }
}

impl fmt::Display for HttpAttemptFailure {
    /// Formats the underlying request failure.
    ///
    /// # Parameters
    /// - `formatter`: Destination formatter.
    ///
    /// # Returns
    /// Result from formatting the underlying error.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(formatter)
    }
}

impl Error for HttpAttemptFailure {
    /// Returns the underlying HTTP error as this wrapper's source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.error.source()
    }
}
