// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Policy controlling whether requests may target another origin.
///
/// The default policy restricts absolute targets when a client base URL is
/// configured and rejects cross-origin redirects from any initial URL.
///
/// # Examples
///
/// ```
/// use qubit_http::HttpClientOptions;
/// use qubit_http::HttpOriginPolicy;
///
/// let mut options = HttpClientOptions::default();
/// assert_eq!(options.origin_policy, HttpOriginPolicy::SameOrigin);
/// options.origin_policy = HttpOriginPolicy::AnyOrigin;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpOriginPolicy {
    /// Require the configured base URL's scheme/host/port, or use the initial
    /// explicit absolute request URL as the origin when no base URL is set.
    #[default]
    SameOrigin,
    /// Permit absolute URLs across origins.
    AnyOrigin,
}
