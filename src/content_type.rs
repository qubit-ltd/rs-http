// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
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
#[must_use]
#[inline]
pub(crate) fn media_type(content_type: &str) -> &str {
    content_type.split(';').next().map(str::trim).unwrap_or_default()
}

/// Returns whether `content_type` has the expected media type.
///
/// # Parameters
/// - `content_type`: Raw Content-Type header value.
/// - `expected`: Expected media type, such as `text/event-stream`.
///
/// # Returns
/// `true` when the media type matches case-insensitively.
#[must_use]
#[inline]
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
#[must_use]
#[inline]
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
#[must_use]
pub(crate) fn is_multipart(content_type: &str) -> bool {
    let Some((kind, subtype)) = media_type(content_type).split_once('/') else {
        return false;
    };
    kind.eq_ignore_ascii_case("multipart") && !subtype.is_empty() && subtype.bytes().all(is_media_type_token_byte)
}

/// Returns whether one byte is allowed in an HTTP media type token.
///
/// # Parameters
/// - `byte`: ASCII byte to validate.
///
/// # Returns
/// `true` for ASCII alphanumeric bytes and the permitted token punctuation.
fn is_media_type_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

/// Returns whether `boundary` is safe for unquoted multipart Content-Type use.
///
/// # Parameters
/// - `boundary`: Boundary parameter value without surrounding quotes.
///
/// # Returns
/// `true` when the boundary is 1 to 70 ASCII token-safe characters.
#[must_use]
#[inline]
pub(crate) fn is_valid_multipart_boundary(boundary: &str) -> bool {
    let len = boundary.len();
    (1..=MAX_MULTIPART_BOUNDARY_LEN).contains(&len) && boundary.bytes().all(is_valid_multipart_boundary_byte)
}

/// Extracts a unique semicolon-separated header parameter.
///
/// # Parameters
/// - `value`: Header value containing parameters.
/// - `parameter_name`: Parameter name to find.
///
/// # Returns
/// Decoded parameter value, or `None` when absent. Returns an error for
/// malformed or duplicate occurrences, including duplicates with equal values.
pub(crate) fn unique_parameter(value: &str, parameter_name: &str) -> Result<Option<String>, &'static str> {
    let segments = header_parameter_segments(value).ok_or("Malformed header parameters")?;
    let mut found = None;
    for segment in segments.into_iter().skip(1) {
        let Some((name, raw_value)) = segment.split_once('=') else {
            if segment.trim().eq_ignore_ascii_case(parameter_name) {
                return Err("Malformed header parameter");
            }
            continue;
        };
        if !name.trim().eq_ignore_ascii_case(parameter_name) {
            continue;
        }
        if found.is_some() {
            return Err("Duplicate header parameter");
        }
        found = Some(decode_header_parameter(raw_value.trim()).ok_or("Malformed header parameter")?);
    }
    Ok(found)
}

/// Returns whether one byte is allowed in generated multipart boundaries.
///
/// # Parameters
/// - `byte`: ASCII byte to validate.
///
/// # Returns
/// `true` for alphanumeric bytes and the unquoted MIME boundary subset.
fn is_valid_multipart_boundary_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' | b'\'' | b'+' | b'_' | b'-' | b'.'
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

#[cfg(test)]
mod tests {
    use super::is_multipart;

    #[test]
    fn test_is_multipart_accepts_nonempty_token_subtypes_and_rejects_invalid_values() {
        for content_type in [
            "multipart/form-data",
            "multipart/mixed",
            "Multipart/FoRm-DaTa; boundary=abc",
            "multipart/foo!#$%&'*+-.^_`|~bar",
        ] {
            assert!(is_multipart(content_type), "rejected {content_type}");
        }

        for content_type in [
            "multipart/",
            "multipart/foo/bar",
            "multipart/foo bar",
            "multipart/foo@bar",
            "multipart/föo",
        ] {
            assert!(!is_multipart(content_type), "accepted {content_type}");
        }
    }
}
