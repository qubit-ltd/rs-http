// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Controls whether complete URL paths remain visible in sanitized log output.
///
/// This policy does not change masking of URL userinfo, fragments, or
/// recognized sensitive query values. Paths are preserved by default for
/// backward compatibility; callers should select [`Self::Redact`] when path
/// segments may contain secrets.
#[non_exhaustive]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum UrlPathPolicy {
    /// Preserves the complete URL path for backward-compatible log output.
    #[default]
    Preserve,
    /// Replaces the complete URL path with a fixed redaction marker.
    Redact,
}
