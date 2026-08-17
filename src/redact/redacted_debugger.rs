// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
//! Safe rendering helpers for `Debug` implementations.

use qubit_redact::RedactionSession;
use qubit_redact::formats::http::HttpRedactor;
use url::Url;

/// Renders diagnostic fields with one immutable policy snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RedactedDebugger<'redactor> {
    /// Unified redactor used for every debug field.
    redactor: &'redactor HttpRedactor,
}

impl<'redactor> RedactedDebugger<'redactor> {
    /// Creates a debugger from the supplied policy without merging defaults.
    ///
    /// # Parameters
    ///
    /// * `log_redactor` - Immutable redactor snapshot to use.
    ///
    /// # Returns
    ///
    /// A safe debug renderer.
    #[inline(always)]
    pub(crate) const fn new(log_redactor: &'redactor HttpRedactor) -> Self {
        Self {
            redactor: log_redactor,
        }
    }

    /// Creates one diagnostic session for a complete debug representation.
    #[inline(always)]
    pub(crate) fn session(&self) -> RedactionSession<'redactor> {
        self.redactor.session()
    }

    /// Returns an optional redacted URL through a shared session.
    #[inline(always)]
    pub(crate) fn optional_url(
        &self,
        url: Option<&Url>,
        session: RedactionSession<'redactor>,
    ) -> (
        RedactionSession<'redactor>,
        Option<qubit_redact::LogSafeText<'static>>,
    ) {
        let Some(url) = url else {
            return (session, None);
        };
        let (session, redacted) =
            crate::redact::http_with!(session, |http| http.redact_url(url));
        (session, Some(redacted))
    }
}
