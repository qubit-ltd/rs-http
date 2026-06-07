// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Sanitized helpers for `Debug` and diagnostic formatting.

use std::collections::BTreeMap;

use http::HeaderMap;
use url::Url;

use super::{
    LogSanitizePolicy,
    LogSanitizer,
};

/// Sanitized rendering helper for debug and diagnostic fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SanitizedDebugger {
    /// Sanitizer configured with debug-safe defaults plus caller policy.
    sanitizer: LogSanitizer,
}

impl SanitizedDebugger {
    /// Creates a debugger from a log sanitization policy.
    ///
    /// # Parameters
    /// - `policy`: Runtime policy whose custom names should be honored.
    ///
    /// # Returns
    /// Debug helper that always keeps built-in sensitive names active.
    pub(crate) fn new(policy: &LogSanitizePolicy) -> Self {
        Self {
            sanitizer: LogSanitizer::for_debug(policy),
        }
    }

    /// Returns a sanitized URL string.
    ///
    /// # Parameters
    /// - `url`: URL to render.
    ///
    /// # Returns
    /// URL with userinfo, fragments, and sensitive query values masked.
    pub(crate) fn url(&self, url: &Url) -> String {
        self.sanitizer.sanitize_url(url)
    }

    /// Returns a sanitized optional URL string.
    ///
    /// # Parameters
    /// - `url`: Optional URL reference.
    ///
    /// # Returns
    /// `Some` with sanitized URL text when present, otherwise `None`.
    pub(crate) fn optional_url(&self, url: Option<&Url>) -> Option<String> {
        url.map(|url| self.url(url))
    }

    /// Returns sanitized headers for structured debug output.
    ///
    /// # Parameters
    /// - `headers`: Header map to render.
    ///
    /// # Returns
    /// Deterministic lowercase header map with sensitive values masked.
    pub(crate) fn headers(
        &self,
        headers: &HeaderMap,
    ) -> BTreeMap<String, Vec<String>> {
        self.sanitizer.sanitize_header_map(headers)
    }

    /// Sanitizes URL-looking tokens inside diagnostic text.
    ///
    /// # Parameters
    /// - `text`: Diagnostic text that may contain absolute URLs.
    ///
    /// # Returns
    /// Text with parseable URL secrets masked.
    pub(crate) fn diagnostic_text(&self, text: &str) -> String {
        self.sanitizer.sanitize_diagnostic_text(text)
    }
}
