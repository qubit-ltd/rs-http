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
    /// Sanitizer configured with built-in sensitive-name defaults plus the
    /// caller policy.
    sanitizer: LogSanitizer,
}

impl SanitizedDebugger {
    /// Creates a debugger from a log sanitization policy.
    ///
    /// # Parameters
    ///
    /// * `policy` - Runtime policy whose custom names should be honored.
    ///
    /// # Returns
    ///
    /// Debug helper that always keeps built-in sensitive names active.
    #[inline(always)]
    pub(crate) fn new(policy: &LogSanitizePolicy) -> Self {
        Self {
            sanitizer: LogSanitizer::for_debug(policy),
        }
    }

    /// Returns a URL string with userinfo, fragment, and recognized sensitive
    /// query values masked.
    ///
    /// # Parameters
    ///
    /// * `url` - URL to render.
    ///
    /// # Returns
    ///
    /// Sanitized URL whose path follows the configured
    /// [`qubit_sanitize::UrlPathPolicy`]. Paths are redacted by default and are
    /// preserved only when callers explicitly select
    /// [`qubit_sanitize::UrlPathPolicy::Preserve`].
    #[inline(always)]
    pub(crate) fn url(&self, url: &Url) -> String {
        self.sanitizer.sanitize_url(url)
    }

    /// Returns a sanitized optional URL string.
    ///
    /// # Parameters
    ///
    /// * `url` - Optional URL reference.
    ///
    /// # Returns
    ///
    /// `Some` with sanitized URL text whose path follows the configured
    /// [`qubit_sanitize::UrlPathPolicy`] when present, otherwise `None`.
    #[inline(always)]
    pub(crate) fn optional_url(&self, url: Option<&Url>) -> Option<String> {
        url.map(|url| self.url(url))
    }

    /// Returns sanitized headers for structured debug output.
    ///
    /// # Parameters
    ///
    /// * `headers` - Header map to render.
    ///
    /// # Returns
    ///
    /// Deterministic lowercase header map. Headers whose names match the
    /// configured sensitive-name policy are masked; other UTF-8 values are
    /// preserved unchanged.
    #[inline(always)]
    pub(crate) fn headers(
        &self,
        headers: &HeaderMap,
    ) -> BTreeMap<String, Vec<String>> {
        self.sanitizer.sanitize_header_map(headers)
    }

    /// Sanitizes URL-looking tokens inside diagnostic text.
    ///
    /// # Parameters
    ///
    /// * `text` - Diagnostic text that may contain absolute URLs.
    ///
    /// # Returns
    ///
    /// Text with parseable URL userinfo, fragments, and recognized sensitive
    /// query values masked. URL paths follow the configured
    /// [`qubit_sanitize::UrlPathPolicy`] and are redacted by default.
    #[inline(always)]
    pub(crate) fn diagnostic_text(&self, text: &str) -> String {
        self.sanitizer.sanitize_diagnostic_text(text)
    }
}
