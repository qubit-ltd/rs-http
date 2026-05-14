/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Content-Type parsing helpers shared by response and SSE code.

/// Returns the media type portion of a `Content-Type` header value.
///
/// # Parameters
/// - `content_type`: Raw Content-Type header value.
///
/// # Returns
/// Trimmed text before the first `;`, or an empty string when the header value
/// has no media type.
pub(crate) fn media_type(content_type: &str) -> &str {
    content_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
}

/// Returns whether `content_type` has the expected media type.
///
/// # Parameters
/// - `content_type`: Raw Content-Type header value.
/// - `expected`: Expected media type, such as `text/event-stream`.
///
/// # Returns
/// `true` when the media type matches case-insensitively.
pub(crate) fn has_media_type(content_type: &str, expected: &str) -> bool {
    media_type(content_type).eq_ignore_ascii_case(expected)
}

/// Returns whether `content_type` is the SSE media type.
///
/// # Parameters
/// - `content_type`: Raw Content-Type header value.
///
/// # Returns
/// `true` when the media type is exactly `text/event-stream`, ignoring ASCII
/// case and allowing parameters after `;`.
pub(crate) fn is_sse(content_type: &str) -> bool {
    has_media_type(content_type, "text/event-stream")
}
