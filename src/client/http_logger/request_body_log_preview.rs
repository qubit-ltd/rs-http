// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private implementation types used by the HTTP logger.

/// Request body preview category used by TRACE logging.
///
/// # Type Parameters
/// - `'a`: Lifetime of borrowed request body bytes.
pub(super) enum RequestBodyLogPreview<'a> {
    /// Borrowed bytes that can be safely previewed without consuming a stream.
    Bytes(
        /// Body bytes borrowed for safe preview rendering.
        &'a [u8],
    ),
    /// No request body is present.
    Empty,
    /// Body logging is intentionally skipped.
    Skipped(
        /// Static reason that body previewing was skipped.
        &'static str,
    ),
}
