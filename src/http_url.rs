// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Helpers for validating HTTP request URLs.

use url::Url;

/// Returns whether `url` uses a supported HTTP transport scheme.
///
/// # Parameters
/// - `url`: URL whose scheme is checked.
///
/// # Returns
/// `true` when the scheme is `http` or `https`.
#[must_use]
#[inline]
pub(crate) fn is_http_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}
