// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Segments retained while preparing a diagnostic message for safe rendering.

use std::ops::Range;

use qubit_redact::DiagnosticRedactionHandle;

/// A plain message slice or a URL whose redacted text is held by a batch.
///
/// Plain ranges use byte offsets into the original message. Their endpoints
/// must be valid UTF-8 character boundaries before they are used for slicing.
pub(in crate::redact) enum MessageSegment {
    /// An unchanged slice of the original message, identified by byte offsets.
    Plain(
        /// Half-open byte range on valid UTF-8 boundaries in the source text.
        Range<usize>,
    ),
    /// A URL replaced by the corresponding result from a diagnostic batch.
    Url(
        /// Handle used to retrieve this URL's redacted diagnostic text.
        DiagnosticRedactionHandle,
    ),
}
