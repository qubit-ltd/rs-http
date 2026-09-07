// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

/// Policy controlling whether requests may target another origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpOriginPolicy {
    /// Require a trusted base URL and the same scheme/host/port.
    #[default]
    SameOrigin,
    /// Permit absolute URLs across origins.
    AnyOrigin,
}
