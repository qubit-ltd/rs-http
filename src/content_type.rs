/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Content-Type and header-parameter parsing helpers.

const MAX_MULTIPART_BOUNDARY_LEN: usize = 70;

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

/// Returns whether a content type declares multipart content.
///
/// # Parameters
/// - `content_type`: Raw Content-Type header value.
///
/// # Returns
/// `true` for any `multipart/*` media type.
pub(crate) fn is_multipart(content_type: &str) -> bool {
    media_type(content_type)
        .to_ascii_lowercase()
        .starts_with("multipart/")
}

/// Returns whether `boundary` is safe for unquoted multipart Content-Type use.
///
/// # Parameters
/// - `boundary`: Boundary parameter value without surrounding quotes.
///
/// # Returns
/// `true` when the boundary is 1 to 70 ASCII token-safe characters.
pub(crate) fn is_valid_multipart_boundary(boundary: &str) -> bool {
    let len = boundary.len();
    (1..=MAX_MULTIPART_BOUNDARY_LEN).contains(&len)
        && boundary.bytes().all(is_valid_multipart_boundary_byte)
}

/// Extracts one semicolon-separated header parameter.
///
/// # Parameters
/// - `value`: Header value containing parameters.
/// - `parameter_name`: Parameter name to find.
///
/// # Returns
/// Decoded parameter value, or `None` when absent or malformed.
pub(crate) fn parameter(value: &str, parameter_name: &str) -> Option<String> {
    for segment in header_parameter_segments(value)?.into_iter().skip(1) {
        let Some((name, raw_value)) = segment.split_once('=') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case(parameter_name) {
            continue;
        }
        return decode_header_parameter(raw_value.trim());
    }
    None
}

/// Returns whether a header value declares a parameter name.
///
/// # Parameters
/// - `value`: Header value containing semicolon-separated parameters.
/// - `parameter_name`: Parameter name to find.
///
/// # Returns
/// `Some(true)` when a `name=value` segment uses `parameter_name`,
/// `Some(false)` when no such parameter exists, or `None` when parameter
/// quoting is malformed.
pub(crate) fn has_parameter_name(value: &str, parameter_name: &str) -> Option<bool> {
    Some(
        header_parameter_segments(value)?
            .into_iter()
            .skip(1)
            .filter_map(|parameter| parameter.split_once('='))
            .any(|(name, _)| name.trim().eq_ignore_ascii_case(parameter_name)),
    )
}

/// Returns whether one byte is allowed in generated multipart boundaries.
///
/// # Parameters
/// - `byte`: ASCII byte to validate.
///
/// # Returns
/// `true` for alphanumeric bytes and conservative RFC-compatible punctuation.
fn is_valid_multipart_boundary_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'0'..=b'9'
            | b'A'..=b'Z'
            | b'a'..=b'z'
            | b'\''
            | b'('
            | b')'
            | b'+'
            | b'_'
            | b','
            | b'-'
            | b'.'
            | b'/'
            | b':'
            | b'='
            | b'?'
    )
}

/// Splits header parameters without treating quoted semicolons as separators.
///
/// # Parameters
/// - `value`: Header value containing semicolon-separated parameters.
///
/// # Returns
/// Parameter segments, or `None` when quotes are malformed.
fn header_parameter_segments(value: &str) -> Option<Vec<&str>> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut in_quote = false;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_quote && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_quote = !in_quote;
            continue;
        }
        if ch == ';' && !in_quote {
            segments.push(value[start..index].trim());
            start = index + ch.len_utf8();
        }
    }
    if in_quote || escaped {
        return None;
    }
    segments.push(value[start..].trim());
    Some(segments)
}

/// Decodes a simple HTTP header parameter value.
///
/// # Parameters
/// - `value`: Raw parameter value.
///
/// # Returns
/// Unquoted value, or `None` for malformed quoted strings.
fn decode_header_parameter(value: &str) -> Option<String> {
    if !value.starts_with('"') {
        return Some(value.trim().to_string());
    }
    if !value.ends_with('"') || value.len() < 2 {
        return None;
    }
    let mut result = String::new();
    let mut chars = value[1..value.len() - 1].chars();
    while let Some(ch) = chars.next() {
        let value = if ch == '\\' { chars.next()? } else { ch };
        if value == '\r' || value == '\n' {
            return None;
        }
        result.push(value);
    }
    Some(result)
}
