// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Fallback source preserving the display message of a retry application error.

use std::error::Error;
use std::fmt;

/// Display-only source used when a retry error has no concrete source chain.
#[must_use = "the fallback source must be attached to the mapped HTTP error"]
#[derive(Debug)]
pub(in crate::client) struct HttpRetryApplicationSource(
    /// Original application error message retained for display.
    String,
);

impl HttpRetryApplicationSource {
    /// Creates a fallback source containing the application's display message.
    ///
    /// # Parameters
    /// - `message`: Original application error message.
    ///
    /// # Returns
    /// A source that displays `message` without exposing another error chain.
    #[inline]
    pub(in crate::client) fn new(message: String) -> Self {
        Self(message)
    }
}

impl fmt::Display for HttpRetryApplicationSource {
    /// Writes the original application error message.
    ///
    /// # Parameters
    /// - `formatter`: Destination for the display text.
    ///
    /// # Returns
    /// The formatter result, including any write failure.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for HttpRetryApplicationSource {}
