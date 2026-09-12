// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

/// Policy controlling whether requests may target another origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpOriginPolicy {
    /// Require the configured base URL's scheme/host/port, or use the initial
    /// explicit absolute request URL as the origin when no base URL is set.
    #[default]
    SameOrigin,
    /// Permit absolute URLs across origins.
    AnyOrigin,
}
