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
pub(in crate::client) struct HttpAttemptFailure {
    /// Public HTTP error produced by the attempt.
    pub(in crate::client) error: HttpError,
    /// Whether the final request permits another attempt.
    pub(in crate::client) retry_allowed: bool,
    /// Whether the final request permits a Retry-After hint.
    pub(in crate::client) honor_retry_after: bool,
}

impl HttpAttemptFailure {
    /// Creates a failure with the final request's retry decisions.
    pub(in crate::client) fn new(error: HttpError, retry_allowed: bool, honor_retry_after: bool) -> Self {
        Self {
            error,
            retry_allowed,
            honor_retry_after,
        }
    }

    /// Consumes this wrapper and returns its HTTP error.
    pub(in crate::client) fn into_error(self) -> HttpError {
        self.error
    }
}

impl fmt::Display for HttpAttemptFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(formatter)
    }
}

impl Error for HttpAttemptFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.error.source()
    }
}
