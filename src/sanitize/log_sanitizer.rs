/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::collections::BTreeMap;

use http::{
    HeaderMap,
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
use crate::content_type;

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

    /// Creates a debug sanitizer that keeps built-in sensitive names active.
    ///
    /// # Parameters
    /// - `policy`: User-visible policy whose custom names should also apply.
    ///
    /// # Returns
    /// Sanitizer that always includes safe built-in defaults plus custom names.
    pub(crate) fn for_debug(policy: &LogSanitizePolicy) -> Self {
        let mut debug_policy = LogSanitizePolicy::default();
        debug_policy
            .sensitive_headers
            .extend(policy.sensitive_headers.iter());
        debug_policy
            .sensitive_query_params
            .extend(policy.sensitive_query_params.iter());
        debug_policy
            .sensitive_body_fields
            .extend(policy.sensitive_body_fields.iter());
        Self::new(debug_policy)
    }

    /// Returns the underlying policy.
    ///
    /// # Returns
    /// Borrowed policy.
    pub fn policy(&self) -> &LogSanitizePolicy {
        &self.policy
    }

    /// Returns a log-safe URL string with sensitive URL components masked.
    ///
    /// # Parameters
    /// - `url`: URL to render.
    ///
    /// # Returns
    /// Sanitized URL string.
    pub fn sanitize_url(&self, url: &Url) -> String {
        let mut sanitized = url.clone();
        if !sanitized.username().is_empty() {
            let _ = sanitized.set_username(SENSITIVE_HEADER_MASK_PLACEHOLDER);
        }
        if sanitized.password().is_some() {
            let _ = sanitized.set_password(Some(SENSITIVE_HEADER_MASK_PLACEHOLDER));
        }
        if sanitized.fragment().is_some() {
            sanitized.set_fragment(Some(SENSITIVE_HEADER_MASK_PLACEHOLDER));
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

    /// Returns log-safe headers for structured debug output.
    ///
    /// # Parameters
    /// - `headers`: Header map to render.
    ///
    /// # Returns
    /// Deterministic map of lowercase header names to sanitized values.
    pub(crate) fn sanitize_header_map(&self, headers: &HeaderMap) -> BTreeMap<String, Vec<String>> {
        let mut result = BTreeMap::<String, Vec<String>>::new();
        for (name, value) in headers {
            result
                .entry(name.as_str().to_string())
                .or_default()
                .push(self.sanitize_header_value(name, value));
        }
        result
    }

    /// Sanitizes URL-looking tokens inside a diagnostic message.
    ///
    /// # Parameters
    /// - `text`: Message that may contain one or more absolute URLs.
    ///
    /// # Returns
    /// Message with parseable URLs sanitized.
    pub(crate) fn sanitize_diagnostic_text(&self, text: &str) -> String {
        text.split_whitespace()
            .map(|token| self.sanitize_diagnostic_token(token))
            .collect::<Vec<_>>()
            .join(" ")
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
        if self.is_multipart_preview(preview) {
            if let Some(text) = self.sanitize_multipart(preview, bytes) {
                return format!("{text}{suffix}");
            }
            return format!("{MULTIPART_BODY_REDACTED}{suffix}");
        }
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
        if preview.content_type.is_some_and(content_type::is_json) {
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
        preview.content_type.is_some_and(content_type::is_ndjson)
    }

    /// Returns whether `preview` should be parsed as form URL encoded data.
    ///
    /// # Parameters
    /// - `preview`: Preview metadata.
    ///
    /// # Returns
    /// `true` when the content type declares a URL-encoded form.
    fn is_form_preview(&self, preview: &BodyPreview<'_>) -> bool {
        preview
            .content_type
            .is_some_and(content_type::is_form_urlencoded)
    }

    /// Returns whether `preview` should be treated as multipart data.
    ///
    /// # Parameters
    /// - `preview`: Preview metadata.
    ///
    /// # Returns
    /// `true` when the content type declares any multipart media type.
    fn is_multipart_preview(&self, preview: &BodyPreview<'_>) -> bool {
        preview.content_type.is_some_and(content_type::is_multipart)
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
        let boundary = content_type::multipart_boundary(content_type)?;
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
        let name = content_disposition.and_then(|value| content_type::parameter(value, "name"));
        let filename = content_disposition.and_then(|value| {
            content_type::parameter(value, "filename")
                .or_else(|| content_type::parameter(value, "filename*"))
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
        if content_type::is_json(content_type) {
            return self
                .sanitize_json(body.as_bytes())
                .unwrap_or_else(|| MULTIPART_PART_REDACTED.to_string());
        }
        if content_type::is_ndjson(content_type) {
            return self
                .sanitize_ndjson(body.as_bytes())
                .unwrap_or_else(|| MULTIPART_PART_REDACTED.to_string());
        }
        if content_type::is_form_urlencoded(content_type) {
            return self.sanitize_form(body.as_bytes());
        }
        if content_type::is_text(content_type) {
            return body.to_string();
        }
        MULTIPART_PART_REDACTED.to_string()
    }

    /// Sanitizes a single whitespace-delimited diagnostic token.
    ///
    /// # Parameters
    /// - `token`: One token from a diagnostic message.
    ///
    /// # Returns
    /// Token with embedded URL credentials and query secrets masked.
    fn sanitize_diagnostic_token(&self, token: &str) -> String {
        let Some(scheme_start) = find_url_scheme_start(token) else {
            return token.to_string();
        };
        let prefix = &token[..scheme_start];
        let mut candidate_end = token.len();
        loop {
            let candidate = &token[scheme_start..candidate_end];
            if let Ok(url) = Url::parse(candidate) {
                let suffix = &token[candidate_end..];
                return format!("{prefix}{}{suffix}", self.sanitize_url(&url));
            }
            let Some((previous, ch)) = previous_char_boundary(token, candidate_end) else {
                return token.to_string();
            };
            if previous <= scheme_start || !is_trimmable_url_suffix(ch) {
                return token.to_string();
            }
            candidate_end = previous;
        }
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

/// Finds the first absolute HTTP URL scheme inside `token`.
///
/// # Parameters
/// - `token`: Diagnostic token to inspect.
///
/// # Returns
/// Byte offset where the scheme starts, or `None`.
fn find_url_scheme_start(token: &str) -> Option<usize> {
    match (token.find("http://"), token.find("https://")) {
        (Some(http), Some(https)) => Some(http.min(https)),
        (Some(http), None) => Some(http),
        (None, Some(https)) => Some(https),
        (None, None) => None,
    }
}

/// Returns the previous UTF-8 character boundary and character.
///
/// # Parameters
/// - `text`: Source text.
/// - `end`: Current byte end offset.
///
/// # Returns
/// Previous byte offset and character, or `None` at the start.
fn previous_char_boundary(text: &str, end: usize) -> Option<(usize, char)> {
    text[..end].char_indices().next_back()
}

/// Returns whether `ch` is punctuation commonly adjacent to a URL in prose.
///
/// # Parameters
/// - `ch`: Candidate trailing character.
///
/// # Returns
/// `true` if the character may be peeled from a failed URL parse attempt.
fn is_trimmable_url_suffix(ch: char) -> bool {
    matches!(
        ch,
        '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '\''
    )
}

/// Kind of multipart delimiter line found in a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MultipartDelimiter {
    /// Delimiter before a regular part.
    Part,
    /// Final closing delimiter.
    Closing,
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
    let mut current_start = None;
    let mut segments = Vec::new();
    let mut position = 0;
    while position < text.len() {
        let (line_start, line_end, next_position) = next_line_bounds(text, position);
        let line = &text[line_start..line_end];
        let Some(delimiter) = multipart_delimiter(line, boundary) else {
            position = next_position;
            continue;
        };
        if let Some(start) = current_start {
            let segment = strip_one_trailing_line_ending(&text[start..line_start]);
            if !segment.trim().is_empty() {
                segments.push(segment);
            }
        }
        if delimiter == MultipartDelimiter::Closing {
            if text[next_position..].trim().is_empty() {
                return Some(segments);
            }
            return None;
        }
        current_start = Some(next_position);
        position = next_position;
    }
    None
}

/// Returns the next line range and the following scan position.
///
/// # Parameters
/// - `text`: Source text.
/// - `position`: Byte offset where the next line starts.
///
/// # Returns
/// `(line_start, line_end_without_line_ending, next_position)`.
fn next_line_bounds(text: &str, position: usize) -> (usize, usize, usize) {
    if let Some(relative_end) = text[position..].find('\n') {
        let line_end = position + relative_end;
        let trimmed_end = line_end
            .checked_sub(1)
            .filter(|index| text.as_bytes()[*index] == b'\r')
            .unwrap_or(line_end);
        return (position, trimmed_end, line_end + 1);
    }
    (position, text.len(), text.len())
}

/// Classifies a multipart boundary delimiter line.
///
/// # Parameters
/// - `line`: One logical line without the trailing CRLF/LF.
/// - `boundary`: Boundary parameter without the leading `--`.
///
/// # Returns
/// Delimiter kind for exact delimiter lines, otherwise `None`.
fn multipart_delimiter(line: &str, boundary: &str) -> Option<MultipartDelimiter> {
    let delimiter = format!("--{boundary}");
    if line == delimiter {
        Some(MultipartDelimiter::Part)
    } else if line == format!("{delimiter}--") {
        Some(MultipartDelimiter::Closing)
    } else {
        None
    }
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
