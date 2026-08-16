// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
//! Presentation-level body preview limits.

use qubit_redact::formats::http::BodyCapture;

/// Describes the caller-selected prefix before hard runtime limits apply.
#[must_use]
#[derive(Debug, Clone, Copy)]
pub(crate) struct BodyPreview<'a> {
    /// Complete source bytes.
    bytes: &'a [u8],
    /// Positive presentation limit.
    limit: usize,
}

impl<'a> BodyPreview<'a> {
    /// Creates a preview and clamps a zero limit to one byte.
    ///
    /// # Parameters
    ///
    /// * `bytes` - Complete source body bytes.
    /// * `limit` - Presentation prefix limit.
    ///
    /// # Returns
    ///
    /// A preview retaining the complete source for exact metadata.
    #[inline(always)]
    pub(crate) const fn new(bytes: &'a [u8], limit: usize) -> Self {
        Self {
            bytes,
            limit: if limit == 0 { 1 } else { limit },
        }
    }

    /// Converts the presentation prefix into truthful capture metadata.
    ///
    /// # Returns
    ///
    /// A complete capture when the source fits, otherwise a truncated capture
    /// with its exact total source length.
    #[inline]
    pub(crate) fn capture(self) -> BodyCapture<'a> {
        BodyCapture::prefix(self.bytes, self.limit)
    }
}
