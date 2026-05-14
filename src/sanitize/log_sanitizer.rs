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

const MULTIPART_BODY_REDACTED: &str = "<redacted: multipart body>";
const MULTIPART_PART_REDACTED: &str = "<redacted: multipart part>";
const MULTIPART_FILE_PART_REDACTED: &str = "<redacted: file part>";
const MULTIPART_UNNAMED_FIELD: &str = "<unnamed>";

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
        if self.is_multipart_preview(preview) {
            if let Some(text) = self.sanitize_multipart(preview, bytes) {
                return format!("{text}{suffix}");
            }
            return format!("{MULTIPART_BODY_REDACTED}{suffix}");
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

    /// Returns whether `preview` should be treated as multipart form data.
    ///
    /// # Parameters
    /// - `preview`: Preview metadata.
    ///
    /// # Returns
    /// `true` when the content type declares multipart form data.
    fn is_multipart_preview(&self, preview: &BodyPreview<'_>) -> bool {
        preview.content_type.is_some_and(is_multipart_content_type)
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

    /// Redacts sensitive fields in one complete multipart body.
    ///
    /// # Parameters
    /// - `preview`: Preview metadata, including content type and truncation state.
    /// - `bytes`: Complete body bytes to parse.
    ///
    /// # Returns
    /// Sanitized multipart summary, or `None` when the body must be fully redacted.
    fn sanitize_multipart(&self, preview: &BodyPreview<'_>, bytes: &[u8]) -> Option<String> {
        if preview.is_truncated() {
            return None;
        }
        let content_type = preview.content_type?;
        let boundary = multipart_boundary(content_type)?;
        let text = std::str::from_utf8(bytes).ok()?;
        let segments = multipart_part_segments(text, &boundary)?;
        let mut lines = Vec::with_capacity(segments.len());
        for segment in segments {
            lines.push(self.sanitize_multipart_part(segment)?);
        }
        if lines.is_empty() {
            return Some("<multipart>\n</multipart>".to_string());
        }
        Some(format!("<multipart>\n{}\n</multipart>", lines.join("\n")))
    }

    /// Redacts one multipart part and renders a log summary line.
    ///
    /// # Parameters
    /// - `segment`: Raw part segment without boundary delimiter lines.
    ///
    /// # Returns
    /// Log-safe `name=value` summary line, or `None` for malformed headers.
    fn sanitize_multipart_part(&self, segment: &str) -> Option<String> {
        let (headers, body) = split_multipart_headers_and_body(segment)?;
        let mut content_disposition = None;
        let mut content_type = None;
        for line in headers.lines().filter(|line| !line.trim().is_empty()) {
            let (header_name, header_value) = line.split_once(':')?;
            let header_name = header_name.trim();
            let header_value = header_value.trim();
            if header_name.eq_ignore_ascii_case("content-disposition") {
                content_disposition = Some(header_value);
            } else if header_name.eq_ignore_ascii_case("content-type") {
                content_type = Some(header_value);
            }
        }
        let name = content_disposition.and_then(|value| header_parameter(value, "name"));
        let filename = content_disposition.and_then(|value| {
            header_parameter(value, "filename").or_else(|| header_parameter(value, "filename*"))
        });
        let field_name = name.as_deref().unwrap_or(MULTIPART_UNNAMED_FIELD);
        let value =
            self.sanitize_multipart_part_value(field_name, filename.as_deref(), content_type, body);
        Some(format!("{field_name}={value}"))
    }

    /// Redacts or renders one multipart part value.
    ///
    /// # Parameters
    /// - `field_name`: Parsed multipart field name.
    /// - `filename`: Optional file name from content disposition.
    /// - `content_type`: Optional part-level content type.
    /// - `body`: Part body text.
    ///
    /// # Returns
    /// Log-safe value for the part.
    fn sanitize_multipart_part_value(
        &self,
        field_name: &str,
        filename: Option<&str>,
        content_type: Option<&str>,
        body: &str,
    ) -> String {
        if self.policy.sensitive_body_fields.contains(field_name) {
            return SENSITIVE_HEADER_MASK_PLACEHOLDER.to_string();
        }
        if filename.is_some() {
            return MULTIPART_FILE_PART_REDACTED.to_string();
        }
        if field_name == MULTIPART_UNNAMED_FIELD {
            return MULTIPART_PART_REDACTED.to_string();
        }
        let Some(content_type) = content_type else {
            return body.to_string();
        };
        if is_json_content_type(content_type) {
            return self
                .sanitize_json(body.as_bytes())
                .unwrap_or_else(|| MULTIPART_PART_REDACTED.to_string());
        }
        if is_ndjson_content_type(content_type) {
            return self
                .sanitize_ndjson(body.as_bytes())
                .unwrap_or_else(|| MULTIPART_PART_REDACTED.to_string());
        }
        if is_form_content_type(content_type) {
            return self.sanitize_form(body.as_bytes());
        }
        if is_text_content_type(content_type) {
            return body.to_string();
        }
        MULTIPART_PART_REDACTED.to_string()
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
    let media_type = content_type_media_type(content_type).to_ascii_lowercase();
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
    let media_type = content_type_media_type(content_type).to_ascii_lowercase();
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
    content_type_media_type(content_type).eq_ignore_ascii_case("application/x-www-form-urlencoded")
}

/// Returns whether a content type declares multipart form data.
///
/// # Parameters
/// - `content_type`: Header value.
///
/// # Returns
/// `true` for `multipart/form-data`.
fn is_multipart_content_type(content_type: &str) -> bool {
    content_type_media_type(content_type).eq_ignore_ascii_case("multipart/form-data")
}

/// Returns whether a content type declares textual data.
///
/// # Parameters
/// - `content_type`: Header value.
///
/// # Returns
/// `true` for `text/*` media types.
fn is_text_content_type(content_type: &str) -> bool {
    content_type_media_type(content_type)
        .to_ascii_lowercase()
        .starts_with("text/")
}

/// Returns the media type part of a content type value.
///
/// # Parameters
/// - `content_type`: Header value.
///
/// # Returns
/// Trimmed media type without parameters.
fn content_type_media_type(content_type: &str) -> &str {
    content_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
}

/// Extracts a valid multipart boundary parameter.
///
/// # Parameters
/// - `content_type`: Multipart content type header value.
///
/// # Returns
/// Boundary string, or `None` when absent or invalid.
fn multipart_boundary(content_type: &str) -> Option<String> {
    if !is_multipart_content_type(content_type) {
        return None;
    }
    let boundary = header_parameter(content_type, "boundary")?;
    if boundary.is_empty() || boundary.chars().any(char::is_control) {
        return None;
    }
    Some(boundary)
}

/// Splits a complete multipart body into raw part segments.
///
/// # Parameters
/// - `text`: Multipart body text.
/// - `boundary`: Boundary parameter without the leading `--`.
///
/// # Returns
/// Raw part segments without boundary delimiter lines, or `None` for malformed bodies.
fn multipart_part_segments<'a>(text: &'a str, boundary: &str) -> Option<Vec<&'a str>> {
    let delimiter = format!("--{boundary}");
    let mut segments = Vec::new();
    let mut split = text.split(&delimiter);
    let _preamble = split.next()?;
    let mut saw_closing_delimiter = false;
    for segment in split {
        if let Some(rest) = segment.strip_prefix("--") {
            if !rest.trim().is_empty() {
                return None;
            }
            saw_closing_delimiter = true;
            break;
        }
        let segment = strip_one_leading_line_ending(segment);
        let segment = strip_one_trailing_line_ending(segment);
        if segment.trim().is_empty() {
            continue;
        }
        segments.push(segment);
    }
    if !saw_closing_delimiter {
        return None;
    }
    Some(segments)
}

/// Splits multipart part headers from the part body.
///
/// # Parameters
/// - `segment`: Raw part segment.
///
/// # Returns
/// Header text and body text.
fn split_multipart_headers_and_body(segment: &str) -> Option<(&str, &str)> {
    if let Some(index) = segment.find("\r\n\r\n") {
        return Some((&segment[..index], &segment[index + 4..]));
    }
    if let Some(index) = segment.find("\n\n") {
        return Some((&segment[..index], &segment[index + 2..]));
    }
    None
}

/// Extracts one semicolon-separated header parameter.
///
/// # Parameters
/// - `value`: Header value containing parameters.
/// - `parameter_name`: Parameter name to find.
///
/// # Returns
/// Decoded parameter value, or `None` when absent or malformed.
fn header_parameter(value: &str, parameter_name: &str) -> Option<String> {
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

/// Removes one leading multipart line ending.
///
/// # Parameters
/// - `value`: Text that may start with a line ending.
///
/// # Returns
/// Text without one leading line ending.
fn strip_one_leading_line_ending(value: &str) -> &str {
    value
        .strip_prefix("\r\n")
        .or_else(|| value.strip_prefix('\n'))
        .unwrap_or(value)
}

/// Removes one trailing multipart line ending.
///
/// # Parameters
/// - `value`: Text that may end with a line ending.
///
/// # Returns
/// Text without one trailing line ending.
fn strip_one_trailing_line_ending(value: &str) -> &str {
    value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(value)
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
