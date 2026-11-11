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

/// Returns whether two URLs use the same web origin.
///
/// # Parameters
/// - `left`: First URL to compare.
/// - `right`: Second URL to compare.
///
/// # Returns
/// `true` when scheme, host, and effective port match.
pub(crate) fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host() == right.host()
        && left.port_or_known_default() == right.port_or_known_default()
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::same_origin;

    #[test]
    fn test_same_origin_comparison() {
        for (left, right, expected) in [
            ("https://a.test/a", "https://a.test/b?q=1", true),
            ("https://a.test", "https://a.test:443", true),
            ("http://a.test", "http://a.test:80", true),
            ("https://a.test", "http://a.test", false),
            ("https://a.test", "https://b.test", false),
            ("https://a.test", "https://a.test:444", false),
            ("http://[::1]", "http://[::1]:80", true),
            ("http://[::1]", "http://[::2]", false),
        ] {
            assert_eq!(
                same_origin(&Url::parse(left).unwrap(), &Url::parse(right).unwrap()),
                expected,
                "{left} vs {right}"
            );
        }
    }
}
