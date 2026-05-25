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
    HeaderMap,
    HeaderName,
    HeaderValue,
};
use qubit_sanitize::{
    FieldSanitizePolicy,
    FieldSanitizer,
    HttpBodySanitizer,
    HttpHeaderSanitizer,
    MaskPolicies,
    NameMatchMode,
    SensitiveFields,
    UrlSanitizer,
};
use url::Url;

use super::{
    BodyLogContext,
    BodyPreview,
    LogSanitizePolicy,
};

const INVALID_CONTENT_TYPE_BODY_REDACTED: &str = "<redacted: invalid content type body>";
const LOG_NAME_MATCH_MODE: NameMatchMode = NameMatchMode::ExactOrSuffix;

/// Applies a [`LogSanitizePolicy`] to URLs, headers, and body previews.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogSanitizer {
    /// Masking and redaction policy.
    policy: LogSanitizePolicy,
    /// URL sanitizer from `qubit-sanitize`.
    url_sanitizer: UrlSanitizer,
    /// Header sanitizer from `qubit-sanitize`.
    header_sanitizer: HttpHeaderSanitizer,
    /// Body sanitizer from `qubit-sanitize`.
    body_sanitizer: HttpBodySanitizer,
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
        Self {
            url_sanitizer: UrlSanitizer::new(field_sanitizer(&policy.sensitive_query_params)),
            header_sanitizer: HttpHeaderSanitizer::new(field_sanitizer(&policy.sensitive_headers)),
            body_sanitizer: HttpBodySanitizer::new(field_sanitizer(&policy.sensitive_body_fields)),
            policy,
        }
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
        extend_sensitive_fields(&mut debug_policy.sensitive_headers, &policy.sensitive_headers);
        extend_sensitive_fields(&mut debug_policy.sensitive_query_params, &policy.sensitive_query_params);
        extend_sensitive_fields(&mut debug_policy.sensitive_body_fields, &policy.sensitive_body_fields);
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
        self.url_sanitizer.sanitize_url(url, LOG_NAME_MATCH_MODE)
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
        self.header_sanitizer.sanitize_value(name, value, LOG_NAME_MATCH_MODE)
    }

    /// Returns log-safe headers for structured debug output.
    ///
    /// # Parameters
    /// - `headers`: Header map to render.
    ///
    /// # Returns
    /// Deterministic map of lowercase header names to sanitized values.
    pub(crate) fn sanitize_header_map(&self, headers: &HeaderMap) -> std::collections::BTreeMap<String, Vec<String>> {
        self.header_sanitizer.sanitize_headers(headers, LOG_NAME_MATCH_MODE)
    }

    /// Returns a log-safe request body preview.
    ///
    /// # Parameters
    /// - `body`: Source request body bytes.
    /// - `limit`: Maximum preview bytes; values below 1 are clamped to 1.
    /// - `content_type`: Optional Content-Type header used for structured redaction.
    ///
    /// # Returns
    /// Sanitized request body preview with request-style truncation suffix.
    pub fn sanitize_request_body_preview(&self, body: &[u8], limit: usize, content_type: Option<&str>) -> String {
        self.sanitize_body_bytes(body, limit, BodyLogContext::Request, content_type)
    }

    /// Returns a log-safe response body preview.
    ///
    /// # Parameters
    /// - `body`: Source response body bytes.
    /// - `limit`: Maximum preview bytes; values below 1 are clamped to 1.
    /// - `content_type`: Optional Content-Type header used for structured redaction.
    ///
    /// # Returns
    /// Sanitized response body preview with response-style truncation suffix.
    pub fn sanitize_response_body_preview(&self, body: &[u8], limit: usize, content_type: Option<&str>) -> String {
        self.sanitize_body_bytes(body, limit, BodyLogContext::Response, content_type)
    }

    /// Returns a log-safe status-error body preview.
    ///
    /// # Parameters
    /// - `body`: Source non-success response body bytes.
    /// - `limit`: Maximum preview bytes; values below 1 are clamped to 1.
    /// - `content_type`: Optional Content-Type header used for structured redaction.
    ///
    /// # Returns
    /// Sanitized error body preview with status-error truncation suffix.
    pub fn sanitize_error_response_body_preview(
        &self,
        body: &[u8],
        limit: usize,
        content_type: Option<&str>,
    ) -> String {
        self.sanitize_body_bytes(body, limit, BodyLogContext::ErrorResponse, content_type)
    }

    /// Sanitizes URL-looking tokens inside a diagnostic message.
    ///
    /// # Parameters
    /// - `text`: Message that may contain one or more absolute URLs.
    ///
    /// # Returns
    /// Message with parseable URLs sanitized.
    pub(crate) fn sanitize_diagnostic_text(&self, text: &str) -> String {
        let mut sanitized = String::with_capacity(text.len());
        let mut token_start = None;
        for (index, ch) in text.char_indices() {
            if ch.is_whitespace() {
                if let Some(start) = token_start.take() {
                    sanitized.push_str(&self.sanitize_diagnostic_token(&text[start..index]));
                }
                sanitized.push(ch);
            } else if token_start.is_none() {
                token_start = Some(index);
            }
        }
        if let Some(start) = token_start {
            sanitized.push_str(&self.sanitize_diagnostic_token(&text[start..]));
        }
        sanitized
    }

    /// Returns a log-safe preview string for body bytes.
    ///
    /// # Parameters
    /// - `preview`: Bounded body bytes and content metadata.
    ///
    /// # Returns
    /// Sanitized preview with context-appropriate truncation marker.
    pub(crate) fn sanitize_body_preview(&self, preview: &BodyPreview<'_>) -> String {
        let content_type = match preview.content_type {
            Some(content_type) => match HeaderValue::from_str(content_type) {
                Ok(content_type) => Some(content_type),
                Err(_) => return Self::invalid_content_type_body(preview),
            },
            None => None,
        };
        let rendered = self.body_sanitizer.sanitize_body_preview(
            preview.prefix(),
            preview.source_len(),
            content_type.as_ref(),
            LOG_NAME_MATCH_MODE,
        );
        Self::normalize_error_truncation_suffix(rendered, preview)
    }

    /// Sanitizes body bytes for one logging call site.
    ///
    /// # Parameters
    /// - `body`: Source body bytes.
    /// - `limit`: Maximum preview bytes.
    /// - `context`: Logging call site that controls truncation wording.
    /// - `content_type`: Optional Content-Type header used for structured redaction.
    ///
    /// # Returns
    /// Sanitized body preview text.
    fn sanitize_body_bytes(
        &self,
        body: &[u8],
        limit: usize,
        context: BodyLogContext,
        content_type: Option<&str>,
    ) -> String {
        let preview = BodyPreview::new(body, limit, context);
        let preview = if let Some(content_type) = content_type {
            preview.with_content_type(content_type)
        } else {
            preview
        };
        self.sanitize_body_preview(&preview)
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
            let (previous, ch) =
                previous_char_boundary(token, candidate_end).expect("candidate end is always after URL scheme start");
            if previous <= scheme_start || !is_trimmable_url_suffix(ch) {
                return token.to_string();
            }
            candidate_end = previous;
        }
    }

    /// Renders an invalid content-type body redaction marker.
    ///
    /// # Parameters
    /// - `preview`: Preview metadata.
    ///
    /// # Returns
    /// Redaction marker with the rs-http truncation suffix.
    fn invalid_content_type_body(preview: &BodyPreview<'_>) -> String {
        format!("{INVALID_CONTENT_TYPE_BODY_REDACTED}{}", preview.truncation_suffix())
    }

    /// Converts `qubit-sanitize` counted truncation suffix to rs-http's
    /// historical status-error suffix.
    ///
    /// # Parameters
    /// - `rendered`: Body text returned by `qubit-sanitize`.
    /// - `preview`: Original preview metadata.
    ///
    /// # Returns
    /// Body text with the suffix expected by status-error diagnostics.
    fn normalize_error_truncation_suffix(rendered: String, preview: &BodyPreview<'_>) -> String {
        if preview.context != BodyLogContext::ErrorResponse || !preview.is_truncated() {
            return rendered;
        }
        let counted = format!(
            "...<truncated {} bytes>",
            preview.source_len().saturating_sub(preview.prefix().len())
        );
        if let Some(prefix) = rendered.strip_suffix(&counted) {
            format!("{prefix}{}", preview.truncation_suffix())
        } else {
            format!("{rendered}{}", preview.truncation_suffix())
        }
    }
}

fn field_sanitizer(fields: &SensitiveFields) -> FieldSanitizer {
    FieldSanitizer::new(FieldSanitizePolicy {
        sensitive_fields: fields.clone(),
        mask_policies: MaskPolicies::default(),
    })
}

fn extend_sensitive_fields(target: &mut SensitiveFields, source: &SensitiveFields) {
    for (field, level) in source.iter() {
        target.insert(field, level);
    }
}

impl Default for LogSanitizer {
    /// Creates a sanitizer using [`LogSanitizePolicy::default`].
    fn default() -> Self {
        Self::new(LogSanitizePolicy::default())
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
    match (
        find_ascii_case_insensitive(token, "http://"),
        find_ascii_case_insensitive(token, "https://"),
    ) {
        (Some(http), Some(https)) => Some(http.min(https)),
        (Some(http), None) => Some(http),
        (None, Some(https)) => Some(https),
        (None, None) => None,
    }
}

/// Finds an ASCII needle inside `text` without requiring matching case.
///
/// # Parameters
/// - `text`: Text to scan.
/// - `needle`: ASCII substring to find.
///
/// # Returns
/// Byte offset of the first match, or `None`.
fn find_ascii_case_insensitive(text: &str, needle: &str) -> Option<usize> {
    let needle = needle.as_bytes();
    if needle.is_empty() || text.len() < needle.len() {
        return None;
    }
    text.as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
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
    matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '\'')
}
