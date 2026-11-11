// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Safe rendering helpers for HTTP TRACE logs.

use qubit_redact::Redactor;

use crate::HttpClientOptions;

/// Provides a redaction policy for HTTP TRACE log data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RedactedLogger {
    /// Unified redactor for URLs, headers, and bodies.
    redactor: Redactor,
}

impl RedactedLogger {
    /// Creates a logger from an explicit redactor snapshot.
    ///
    /// # Parameters
    ///
    /// * `log_redactor` - Exact immutable redactor snapshot.
    ///
    /// # Returns
    ///
    /// A safe TRACE rendering helper.
    #[inline]
    pub(crate) fn new(log_redactor: Redactor) -> Self {
        Self { redactor: log_redactor }
    }

    /// Creates a logger for a client option snapshot using its shared redactor.
    ///
    /// # Parameters
    ///
    /// * `options` - Client options associated with this logger.
    /// * `log_redactor` - Exact shared redactor owned by the client lifecycle.
    ///
    /// # Returns
    ///
    /// A helper using the supplied immutable redactor snapshot.
    #[inline]
    pub(crate) fn from_options_with_redactor(_options: &HttpClientOptions, log_redactor: Redactor) -> Self {
        Self::new(log_redactor)
    }

    /// Creates one diagnostic session for a complete TRACE record.
    #[inline]
    pub(crate) const fn redactor(&self) -> &Redactor {
        &self.redactor
    }
}
