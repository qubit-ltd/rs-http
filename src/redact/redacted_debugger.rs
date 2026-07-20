// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Safe rendering helpers for `Debug` implementations.

use http::HeaderMap;
use qubit_redact::http::RedactedHeaders;
use url::Url;

use super::{
    LogRedactionPolicy,
    LogRedactor,
};

/// Renders diagnostic fields with one immutable policy snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RedactedDebugger {
    /// Unified redactor used for every debug field.
    redactor: LogRedactor,
}

impl RedactedDebugger {
    /// Creates a debugger from the supplied policy without merging defaults.
    ///
    /// # Parameters
    ///
    /// * `policy` - Exact immutable policy snapshot to use.
    ///
    /// # Returns
    ///
    /// A safe debug renderer.
    #[inline(always)]
    pub(crate) fn new(policy: &LogRedactionPolicy) -> Self {
        Self {
            redactor: LogRedactor::new(policy.clone()),
        }
    }

    /// Returns a redacted URL representation.
    ///
    /// # Parameters
    ///
    /// * `url` - Parsed URL to redact.
    ///
    /// # Returns
    ///
    /// An owned log-safe URL.
    #[inline(always)]
    pub(crate) fn url(&self, url: &Url) -> qubit_redact::LogSafeText<'static> {
        self.redactor.redact_url(url)
    }

    /// Returns a redacted URL when one is present.
    ///
    /// # Parameters
    ///
    /// * `url` - Optional parsed URL.
    ///
    /// # Returns
    ///
    /// `Some` with log-safe text when present, otherwise `None`.
    #[inline(always)]
    pub(crate) fn optional_url(
        &self,
        url: Option<&Url>,
    ) -> Option<qubit_redact::LogSafeText<'static>> {
        url.map(|url| self.url(url))
    }

    /// Returns an opaque safe rendering of all headers.
    ///
    /// # Parameters
    ///
    /// * `headers` - Header map to redact.
    ///
    /// # Returns
    ///
    /// A deterministic safe header rendering.
    #[inline(always)]
    pub(crate) fn headers(&self, headers: &HeaderMap) -> RedactedHeaders {
        self.redactor.redact_headers(headers)
    }

    /// Redacts URL-looking tokens inside diagnostic text.
    ///
    /// # Parameters
    ///
    /// * `text` - Diagnostic text to inspect.
    ///
    /// # Returns
    ///
    /// Text with recognized HTTP URLs redacted.
    #[inline(always)]
    pub(crate) fn diagnostic_text(&self, text: &str) -> String {
        self.redactor.redact_diagnostic_text(text)
    }
}
