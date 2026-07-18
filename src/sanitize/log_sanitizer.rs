// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use http::{
    HeaderMap,
    HeaderName,
    HeaderValue,
};
use qubit_sanitize::{
    escape_log_control_characters,
    BodySanitization,
    FieldSanitizer,
    HttpBodySanitizer,
    HttpHeaderSanitizer,
    NameMatchMode,
    UrlSanitizer,
};
use url::Url;

use super::{
    BodyLogContext,
    BodyPreview,
    LogSanitizePolicy,
};

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
    ///
    /// * `policy` - Sanitization rules.
    ///
    /// # Returns
    ///
    /// New [`LogSanitizer`].
    pub fn new(policy: LogSanitizePolicy) -> Self {
        Self {
            url_sanitizer: UrlSanitizer::new(FieldSanitizer::new(
                policy.query_param_policy().clone(),
            ))
            .with_url_path_policy(policy.url_path_policy()),
            header_sanitizer: HttpHeaderSanitizer::new(FieldSanitizer::new(
                policy.header_policy().clone(),
            )),
            body_sanitizer: HttpBodySanitizer::new(FieldSanitizer::new(
                policy.body_field_policy().clone(),
            ))
            .with_text_body_policy(policy.text_body_policy()),
            policy,
        }
    }

    /// Creates a debug sanitizer that starts with built-in sensitive names.
    ///
    /// # Parameters
    ///
    /// * `policy` - User-visible policy whose custom names should also apply.
    ///
    /// # Returns
    ///
    /// Sanitizer that combines built-in defaults with custom names from
    /// `policy`, then honors its explicit exclusions.
    pub(crate) fn for_debug(policy: &LogSanitizePolicy) -> Self {
        let mut debug_policy = LogSanitizePolicy::default();
        for (name, level) in policy.header_policy().sensitive_fields().iter() {
            debug_policy.insert_sensitive_header(name, level);
        }
        for (name, level) in
            policy.query_param_policy().sensitive_fields().iter()
        {
            debug_policy.insert_sensitive_query_param(name, level);
        }
        for (name, level) in
            policy.body_field_policy().sensitive_fields().iter()
        {
            debug_policy.insert_sensitive_body_field(name, level);
        }
        policy.apply_exclusions_to(&mut debug_policy);
        debug_policy.set_text_body_policy(policy.text_body_policy());
        debug_policy.set_url_path_policy(policy.url_path_policy());
        Self::new(debug_policy)
    }

    /// Returns the underlying policy.
    ///
    /// # Returns
    ///
    /// Borrowed policy.
    #[inline(always)]
    pub fn policy(&self) -> &LogSanitizePolicy {
        &self.policy
    }

    /// Returns a URL string with userinfo, fragments, and recognized sensitive
    /// query values masked. The path follows the configured
    /// [`qubit_sanitize::UrlPathPolicy`].
    ///
    /// # Parameters
    ///
    /// * `url` - URL to render.
    ///
    /// # Returns
    ///
    /// Sanitized URL string.
    #[inline(always)]
    pub fn sanitize_url(&self, url: &Url) -> String {
        self.url_sanitizer.sanitize_url(url, LOG_NAME_MATCH_MODE)
    }

    /// Renders a header value according to the configured sensitive-name
    /// policy.
    ///
    /// # Parameters
    ///
    /// * `name` - Header name.
    /// * `value` - Header value.
    ///
    /// # Returns
    ///
    /// Masked value when the header name matches the configured sensitive-name
    /// policy, original value for other UTF-8 headers, or `<non-utf8>` when
    /// the header value is not valid UTF-8.
    #[inline(always)]
    pub fn sanitize_header_value(
        &self,
        name: &HeaderName,
        value: &HeaderValue,
    ) -> String {
        self.header_sanitizer
            .sanitize_value(name, value, LOG_NAME_MATCH_MODE)
    }

    /// Returns a request body preview rendered according to the configured
    /// body sanitization policy.
    ///
    /// Selecting
    /// [`TextBodyPolicy::PassThrough`](qubit_sanitize::TextBodyPolicy::PassThrough)
    /// may expose secrets from the original opaque text. Log-unsafe characters
    /// are escaped in the returned rendering.
    ///
    /// # Parameters
    ///
    /// * `body` - Source request body bytes.
    /// * `limit` - Maximum preview bytes; values below 1 are clamped to 1.
    /// * `content_type` - Optional Content-Type header used for structured
    ///   redaction.
    ///
    /// # Returns
    ///
    /// Policy-rendered request body preview with escaped log-unsafe characters
    /// and a request-style truncation suffix.
    #[inline(always)]
    pub fn sanitize_request_body_preview(
        &self,
        body: &[u8],
        limit: usize,
        content_type: Option<&str>,
    ) -> String {
        self.sanitize_body_bytes(
            body,
            limit,
            BodyLogContext::Request,
            content_type,
        )
    }

    /// Returns a response body preview rendered according to the configured
    /// body sanitization policy.
    ///
    /// Selecting
    /// [`TextBodyPolicy::PassThrough`](qubit_sanitize::TextBodyPolicy::PassThrough)
    /// may expose secrets from the original opaque text. Log-unsafe characters
    /// are escaped in the returned rendering.
    ///
    /// # Parameters
    ///
    /// * `body` - Source response body bytes.
    /// * `limit` - Maximum preview bytes; values below 1 are clamped to 1.
    /// * `content_type` - Optional Content-Type header used for structured
    ///   redaction.
    ///
    /// # Returns
    ///
    /// Policy-rendered response body preview with escaped log-unsafe characters
    /// and a response-style truncation suffix.
    #[inline(always)]
    pub fn sanitize_response_body_preview(
        &self,
        body: &[u8],
        limit: usize,
        content_type: Option<&str>,
    ) -> String {
        self.sanitize_body_bytes(
            body,
            limit,
            BodyLogContext::Response,
            content_type,
        )
    }

    /// Returns a status-error body preview rendered according to the
    /// configured body sanitization policy.
    ///
    /// Selecting
    /// [`TextBodyPolicy::PassThrough`](qubit_sanitize::TextBodyPolicy::PassThrough)
    /// may expose secrets from the original opaque text. Log-unsafe characters
    /// are escaped in the returned rendering.
    ///
    /// # Parameters
    ///
    /// * `body` - Source non-success response body bytes.
    /// * `limit` - Maximum preview bytes; values below 1 are clamped to 1.
    /// * `content_type` - Optional Content-Type header used for structured
    ///   redaction.
    ///
    /// # Returns
    ///
    /// Policy-rendered error body preview with escaped log-unsafe characters
    /// and a status-error truncation suffix.
    #[inline(always)]
    pub fn sanitize_error_response_body_preview(
        &self,
        body: &[u8],
        limit: usize,
        content_type: Option<&str>,
    ) -> String {
        self.sanitize_body_bytes(
            body,
            limit,
            BodyLogContext::ErrorResponse,
            content_type,
        )
    }

    /// Renders headers for structured debug output according to the configured
    /// sensitive-name policy.
    ///
    /// # Parameters
    ///
    /// * `headers` - Header map to render.
    ///
    /// # Returns
    ///
    /// Deterministic map of lowercase header names to values. Headers whose
    /// names match the configured sensitive-name policy are masked; other
    /// UTF-8 header values are preserved unchanged.
    #[inline(always)]
    pub(crate) fn sanitize_header_map(
        &self,
        headers: &HeaderMap,
    ) -> std::collections::BTreeMap<String, Vec<String>> {
        self.header_sanitizer
            .sanitize_headers(headers, LOG_NAME_MATCH_MODE)
    }

    /// Sanitizes URL-looking tokens inside a diagnostic message.
    ///
    /// # Parameters
    ///
    /// * `text` - Message that may contain one or more absolute URLs.
    ///
    /// # Returns
    ///
    /// Message with parseable URL userinfo, fragments, and recognized
    /// sensitive query values masked. URL paths follow the configured
    /// [`qubit_sanitize::UrlPathPolicy`] and are redacted by default.
    pub(crate) fn sanitize_diagnostic_text(&self, text: &str) -> String {
        let mut sanitized = String::with_capacity(text.len());
        let mut token_start = None;
        for (index, ch) in text.char_indices() {
            if ch.is_whitespace() {
                if let Some(start) = token_start.take() {
                    sanitized.push_str(
                        &self.sanitize_diagnostic_token(&text[start..index]),
                    );
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

    /// Renders body bytes according to the configured body sanitization
    /// policy.
    ///
    /// Selecting
    /// [`TextBodyPolicy::PassThrough`](qubit_sanitize::TextBodyPolicy::PassThrough)
    /// may expose secrets from the original opaque text. Log-unsafe characters
    /// are escaped in the returned rendering.
    ///
    /// # Parameters
    ///
    /// * `preview` - Bounded body bytes and content metadata.
    ///
    /// # Returns
    ///
    /// Policy-rendered preview with escaped log-unsafe characters and a
    /// context-appropriate truncation marker.
    pub(crate) fn sanitize_body_preview(
        &self,
        preview: &BodyPreview<'_>,
    ) -> String {
        let result = self.body_sanitizer.sanitize_body_preview(
            preview.prefix(),
            preview.source_length(),
            preview.content_type,
            LOG_NAME_MATCH_MODE,
        );
        Self::render_body_sanitization(result, preview)
    }

    /// Renders one structured body sanitization result for an HTTP log context.
    ///
    /// # Parameters
    ///
    /// * `result` - Structured result returned by `qubit-sanitize`.
    /// * `preview` - Preview metadata controlling truncation wording.
    ///
    /// # Returns
    ///
    /// Escaped body text with the context-appropriate truncation suffix.
    fn render_body_sanitization(
        result: BodySanitization,
        preview: &BodyPreview<'_>,
    ) -> String {
        if preview.context == BodyLogContext::ErrorResponse
            && result.is_truncated()
        {
            let rendered = format!(
                "{}{}",
                result.into_content(),
                preview.truncation_suffix()
            );
            escape_log_control_characters(&rendered).into_owned()
        } else {
            result.into_rendered()
        }
    }

    /// Sanitizes body bytes for one logging call site.
    ///
    /// # Parameters
    ///
    /// * `body` - Source body bytes.
    /// * `limit` - Maximum preview bytes.
    /// * `context` - Logging call site that controls truncation wording.
    /// * `content_type` - Optional Content-Type header used for structured
    ///   redaction.
    ///
    /// # Returns
    ///
    /// Policy-rendered body preview text. With
    /// [`TextBodyPolicy::PassThrough`](qubit_sanitize::TextBodyPolicy::PassThrough),
    /// opaque text may expose original secrets after log-unsafe characters are
    /// escaped.
    fn sanitize_body_bytes(
        &self,
        body: &[u8],
        limit: usize,
        context: BodyLogContext,
        content_type: Option<&str>,
    ) -> String {
        let preview = BodyPreview::new(body, limit, context);
        let result = self
            .body_sanitizer
            .sanitize_body_preview_with_content_type_str(
                preview.prefix(),
                preview.source_length(),
                content_type,
                LOG_NAME_MATCH_MODE,
            );
        Self::render_body_sanitization(result, &preview)
    }

    /// Sanitizes a single whitespace-delimited diagnostic token.
    ///
    /// # Parameters
    ///
    /// * `token` - One token from a diagnostic message.
    ///
    /// # Returns
    ///
    /// Token with embedded URL userinfo, fragment, and recognized sensitive
    /// query values masked. Its URL path follows the configured
    /// [`qubit_sanitize::UrlPathPolicy`] and is redacted by default.
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
            let (previous, ch) = previous_char_boundary(token, candidate_end)
                .expect("candidate end is always after URL scheme start");
            if previous <= scheme_start || !is_trimmable_url_suffix(ch) {
                let suffix = &token[candidate_end..];
                return format!(
                    "{prefix}{}{suffix}",
                    self.url_sanitizer.sanitize_url_str_or_redact(
                        candidate,
                        LOG_NAME_MATCH_MODE,
                    ),
                );
            }
            candidate_end = previous;
        }
    }
}

impl Default for LogSanitizer {
    /// Creates a sanitizer using [`LogSanitizePolicy::default`].
    ///
    /// # Returns
    ///
    /// A sanitizer configured with the default log sanitization policy.
    #[inline(always)]
    fn default() -> Self {
        Self::new(LogSanitizePolicy::default())
    }
}

/// Finds the first absolute HTTP URL scheme inside `token`.
///
/// # Parameters
///
/// * `token` - Diagnostic token to inspect.
///
/// # Returns
///
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
///
/// * `text` - Text to scan.
/// * `needle` - ASCII substring to find.
///
/// # Returns
///
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
///
/// * `text` - Source text.
/// * `end` - Current byte end offset.
///
/// # Returns
///
/// Previous byte offset and character, or `None` at the start.
///
/// # Panics
///
/// Panics when `end` exceeds `text.len()` or is not a UTF-8 character
/// boundary.
#[inline(always)]
fn previous_char_boundary(text: &str, end: usize) -> Option<(usize, char)> {
    text[..end].char_indices().next_back()
}

/// Returns whether `ch` is punctuation commonly adjacent to a URL in prose.
///
/// # Parameters
///
/// * `ch` - Candidate trailing character.
///
/// # Returns
///
/// `true` if the character may be peeled from a failed URL parse attempt.
#[inline]
fn is_trimmable_url_suffix(ch: char) -> bool {
    matches!(
        ch,
        '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '\''
    )
}
