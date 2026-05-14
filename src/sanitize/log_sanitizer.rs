/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use http::{
    HeaderName,
    HeaderValue,
};
use serde_json::Value;
use url::{
    form_urlencoded,
    Url,
};

use crate::constants::{
    SENSITIVE_HEADER_MASK_EDGE_CHARS,
    SENSITIVE_HEADER_MASK_PLACEHOLDER,
    SENSITIVE_HEADER_MASK_SHORT_LEN,
};

use super::{
    BodyPreview,
    LogSanitizePolicy,
};

/// Applies a [`LogSanitizePolicy`] to URLs, headers, and body previews.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogSanitizer {
    /// Masking and redaction policy.
    policy: LogSanitizePolicy,
}

impl LogSanitizer {
    /// Creates a sanitizer from an explicit policy.
    ///
    /// # Parameters
    /// - `policy`: Sanitization rules.
    ///
    /// # Returns
    /// New [`LogSanitizer`].
    pub fn new(policy: LogSanitizePolicy) -> Self {
        Self { policy }
    }

    /// Returns the underlying policy.
    ///
    /// # Returns
    /// Borrowed policy.
    pub fn policy(&self) -> &LogSanitizePolicy {
        &self.policy
    }

    /// Returns a log-safe URL string with sensitive query parameters masked.
    ///
    /// # Parameters
    /// - `url`: URL to render.
    ///
    /// # Returns
    /// Sanitized URL string.
    pub fn sanitize_url(&self, url: &Url) -> String {
        let mut sanitized = url.clone();
        if sanitized.password().is_some() {
            let _ = sanitized.set_password(Some(SENSITIVE_HEADER_MASK_PLACEHOLDER));
        }
        let Some(_) = sanitized.query() else {
            return sanitized.to_string();
        };

        let mut serializer = form_urlencoded::Serializer::new(String::new());
        for (key, value) in url.query_pairs() {
            if self.policy.sensitive_query_params.contains(key.as_ref()) {
                serializer.append_pair(key.as_ref(), SENSITIVE_HEADER_MASK_PLACEHOLDER);
            } else {
                serializer.append_pair(key.as_ref(), value.as_ref());
            }
        }
        sanitized.set_query(Some(&serializer.finish()));
        sanitized.to_string()
    }

    /// Returns a log-safe header value.
    ///
    /// # Parameters
    /// - `name`: Header name.
    /// - `value`: Header value.
    ///
    /// # Returns
    /// Masked value for sensitive headers, original value for non-sensitive
    /// UTF-8 values, or `<non-utf8>` when header value is not valid UTF-8.
    pub fn sanitize_header_value(&self, name: &HeaderName, value: &HeaderValue) -> String {
        let value = value.to_str().unwrap_or("<non-utf8>");
        if value.is_empty() {
            return String::new();
        }
        if !self.policy.sensitive_headers.contains(name.as_str()) {
            return value.to_string();
        }
        mask_sensitive_value(value)
    }

    /// Returns a log-safe preview string for body bytes.
    ///
    /// # Parameters
    /// - `preview`: Bounded body bytes and content metadata.
    ///
    /// # Returns
    /// Sanitized preview with context-appropriate truncation marker.
    pub fn sanitize_body_preview(&self, preview: &BodyPreview<'_>) -> String {
        let bytes = preview.prefix();
        if bytes.is_empty() {
            return "<empty>".to_string();
        }
        let suffix = preview.truncation_suffix();
        if self.is_ndjson_preview(preview) {
            if let Some(text) = self.sanitize_ndjson(bytes) {
                return format!("{text}{suffix}");
            }
            return format!("<redacted: invalid or truncated NDJSON>{suffix}");
        }
        if self.is_json_preview(preview, bytes) {
            if let Some(text) = self.sanitize_json(bytes) {
                return format!("{text}{suffix}");
            }
            return format!("<redacted: invalid or truncated JSON>{suffix}");
        }
        if self.is_form_preview(preview) {
            return format!("{}{}", self.sanitize_form(bytes), suffix);
        }
        match std::str::from_utf8(bytes) {
            Ok(text) => format!("{text}{suffix}"),
            Err(_) => format!("<binary {} bytes>{suffix}", preview.source_len()),
        }
    }

    /// Returns whether `preview` should be parsed as JSON.
    ///
    /// # Parameters
    /// - `preview`: Preview metadata.
    /// - `bytes`: Prefix bytes.
    ///
    /// # Returns
    /// `true` when the content type declares JSON or the bytes look like JSON.
    fn is_json_preview(&self, preview: &BodyPreview<'_>, bytes: &[u8]) -> bool {
        if preview.content_type.is_some_and(is_json_content_type) {
            return true;
        }
        let trimmed = trim_ascii_whitespace(bytes);
        matches!(trimmed.first(), Some(b'{') | Some(b'['))
    }

    /// Returns whether `preview` should be parsed as newline-delimited JSON.
    ///
    /// # Parameters
    /// - `preview`: Preview metadata.
    ///
    /// # Returns
    /// `true` when the content type declares NDJSON.
    fn is_ndjson_preview(&self, preview: &BodyPreview<'_>) -> bool {
        preview.content_type.is_some_and(is_ndjson_content_type)
    }

    /// Returns whether `preview` should be parsed as form URL encoded data.
    ///
    /// # Parameters
    /// - `preview`: Preview metadata.
    ///
    /// # Returns
    /// `true` when the content type declares a URL-encoded form.
    fn is_form_preview(&self, preview: &BodyPreview<'_>) -> bool {
        preview.content_type.is_some_and(is_form_content_type)
    }

    /// Redacts sensitive JSON object keys.
    ///
    /// # Parameters
    /// - `bytes`: UTF-8 JSON bytes.
    ///
    /// # Returns
    /// Sanitized compact JSON text, or `None` when parsing/rendering fails.
    fn sanitize_json(&self, bytes: &[u8]) -> Option<String> {
        let mut value = serde_json::from_slice::<Value>(bytes).ok()?;
        self.redact_json_value(&mut value);
        serde_json::to_string(&value).ok()
    }

    /// Redacts sensitive keys in newline-delimited JSON.
    ///
    /// # Parameters
    /// - `bytes`: UTF-8 NDJSON bytes.
    ///
    /// # Returns
    /// Sanitized NDJSON text, or `None` when any non-empty line fails to parse.
    fn sanitize_ndjson(&self, bytes: &[u8]) -> Option<String> {
        let text = std::str::from_utf8(bytes).ok()?;
        let trailing_newline = text.ends_with('\n');
        let mut sanitized_lines = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                sanitized_lines.push(String::new());
                continue;
            }
            let mut value = serde_json::from_str::<Value>(line).ok()?;
            self.redact_json_value(&mut value);
            sanitized_lines.push(serde_json::to_string(&value).ok()?);
        }
        let mut result = sanitized_lines.join("\n");
        if trailing_newline {
            result.push('\n');
        }
        Some(result)
    }

    /// Redacts sensitive keys recursively in one JSON value.
    ///
    /// # Parameters
    /// - `value`: JSON value to mutate in place.
    fn redact_json_value(&self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, value) in map.iter_mut() {
                    if self.policy.sensitive_body_fields.contains(key) {
                        *value = Value::String(SENSITIVE_HEADER_MASK_PLACEHOLDER.to_string());
                    } else {
                        self.redact_json_value(value);
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.redact_json_value(item);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }

    /// Redacts sensitive URL-encoded form fields.
    ///
    /// # Parameters
    /// - `bytes`: Form URL encoded bytes.
    ///
    /// # Returns
    /// Sanitized form body text.
    fn sanitize_form(&self, bytes: &[u8]) -> String {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        for (key, value) in form_urlencoded::parse(bytes) {
            if self.policy.sensitive_body_fields.contains(key.as_ref()) {
                serializer.append_pair(key.as_ref(), SENSITIVE_HEADER_MASK_PLACEHOLDER);
            } else {
                serializer.append_pair(key.as_ref(), value.as_ref());
            }
        }
        serializer.finish()
    }
}

impl Default for LogSanitizer {
    /// Creates a sanitizer using [`LogSanitizePolicy::default`].
    fn default() -> Self {
        Self::new(LogSanitizePolicy::default())
    }
}

/// Masks one sensitive string using the crate's edge-preserving convention.
///
/// # Parameters
/// - `value`: Sensitive value.
///
/// # Returns
/// Masked value.
fn mask_sensitive_value(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= SENSITIVE_HEADER_MASK_SHORT_LEN {
        SENSITIVE_HEADER_MASK_PLACEHOLDER.to_string()
    } else {
        let edge = SENSITIVE_HEADER_MASK_EDGE_CHARS;
        let prefix: String = chars[..edge].iter().collect();
        let suffix: String = chars[chars.len() - edge..].iter().collect();
        format!("{prefix}{SENSITIVE_HEADER_MASK_PLACEHOLDER}{suffix}")
    }
}

/// Returns whether a content type declares JSON.
///
/// # Parameters
/// - `content_type`: Header value.
///
/// # Returns
/// `true` for `application/json` and `*+json` media types.
fn is_json_content_type(content_type: &str) -> bool {
    let media_type = content_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    media_type == "application/json"
        || media_type.ends_with("+json")
        || media_type.ends_with("/json")
}

/// Returns whether a content type declares NDJSON.
///
/// # Parameters
/// - `content_type`: Header value.
///
/// # Returns
/// `true` for `application/x-ndjson` and compatible aliases.
fn is_ndjson_content_type(content_type: &str) -> bool {
    let media_type = content_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    media_type == "application/x-ndjson" || media_type == "application/ndjson"
}

/// Returns whether a content type declares URL-encoded form data.
///
/// # Parameters
/// - `content_type`: Header value.
///
/// # Returns
/// `true` for `application/x-www-form-urlencoded`.
fn is_form_content_type(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .eq_ignore_ascii_case("application/x-www-form-urlencoded")
}

/// Trims ASCII whitespace from both ends of `bytes`.
///
/// # Parameters
/// - `bytes`: Bytes to trim.
///
/// # Returns
/// Borrowed trimmed slice.
fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| index + 1)
        .unwrap_or(start);
    &bytes[start..end]
}
