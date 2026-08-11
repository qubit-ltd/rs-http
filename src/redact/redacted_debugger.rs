// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
//! Safe rendering helpers for `Debug` implementations.

use http::HeaderMap;
use qubit_redact::RedactionSession;
use qubit_redact::http::HttpRedactor;
use qubit_redact::http::RedactedHeaders;
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
        RedactionSession::diagnostic(self.redactor.policy())
    }

    /// Returns a redacted URL through a shared diagnostic session.
    #[inline(always)]
    pub(crate) fn url_with_session(
        &self,
        url: &Url,
        session: &RedactionSession<'_>,
    ) -> qubit_redact::LogSafeText<'static> {
        self.redactor.redact_url_with_session(url, session)
    }

    /// Returns an optional redacted URL through a shared session.
    #[inline(always)]
    pub(crate) fn optional_url_with_session(
        &self,
        url: Option<&Url>,
        session: &RedactionSession<'_>,
    ) -> Option<qubit_redact::LogSafeText<'static>> {
        url.map(|url| self.url_with_session(url, session))
    }

    /// Returns redacted headers through a shared diagnostic session.
    #[inline(always)]
    pub(crate) fn headers_with_session(
        &self,
        headers: &HeaderMap,
        session: &RedactionSession<'_>,
    ) -> RedactedHeaders {
        self.redactor.redact_headers_with_session(headers, session)
    }

    /// Redacts diagnostic text through a shared diagnostic session.
    #[inline(always)]
    pub(crate) fn diagnostic_text_with_session(
        &self,
        text: &str,
        session: &RedactionSession<'_>,
    ) -> qubit_redact::LogSafeText<'static> {
        self.redactor
            .redact_urls_in_text_with_session(text, session)
    }
}
