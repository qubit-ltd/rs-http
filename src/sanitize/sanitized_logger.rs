// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Sanitized rendering helpers for HTTP TRACE logs.

use http::{
    HeaderName,
    HeaderValue,
};
use url::Url;

use crate::HttpClientOptions;

use super::{
    BodyLogContext,
    BodyPreview,
    LogSanitizePolicy,
    LogSanitizer,
};

/// Applies configured sanitization rules to URL, header, and body log values.
///
/// URL userinfo, fragments, and recognized sensitive query values are masked.
/// URL paths follow [`qubit_sanitize::UrlPathPolicy`] and use
/// [`qubit_sanitize::UrlPathPolicy::Redact`] by default. Header and body
/// rendering retain the boundaries documented by their respective methods.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SanitizedLogger {
    /// Sanitizer configured from client logging policy.
    sanitizer: LogSanitizer,
    /// Maximum body bytes included in one preview.
    body_size_limit: usize,
}

impl SanitizedLogger {
    /// Creates a sanitized logger from an explicit policy and body limit.
    ///
    /// # Parameters
    ///
    /// * `policy` - Sanitization policy used for URL/header/body values.
    /// * `body_size_limit` - Maximum bytes rendered from body previews.
    ///
    /// # Returns
    ///
    /// New sanitized logger helper.
    #[inline]
    pub(crate) fn new(
        policy: LogSanitizePolicy,
        body_size_limit: usize,
    ) -> Self {
        Self {
            sanitizer: LogSanitizer::new(policy),
            body_size_limit,
        }
    }

    /// Creates a sanitized logger from client options.
    ///
    /// # Parameters
    ///
    /// * `options` - Client options carrying logging and sanitization policy.
    ///
    /// # Returns
    ///
    /// Sanitized logger configured like [`crate::HttpLogger`].
    #[inline(always)]
    pub(crate) fn from_options(options: &HttpClientOptions) -> Self {
        Self::new(
            options.log_sanitize_policy.clone(),
            options.logging.body_size_limit,
        )
    }

    /// Returns a URL string with userinfo, fragment, and recognized sensitive
    /// query values masked.
    ///
    /// # Parameters
    ///
    /// * `url` - URL to render.
    ///
    /// # Returns
    ///
    /// Sanitized URL whose path follows the configured
    /// [`qubit_sanitize::UrlPathPolicy`]. Paths are redacted by default and are
    /// preserved only when callers explicitly select
    /// [`qubit_sanitize::UrlPathPolicy::Preserve`].
    #[inline(always)]
    pub(crate) fn url(&self, url: &Url) -> String {
        self.sanitizer.sanitize_url(url)
    }

    /// Renders a header value according to its native flag and configured
    /// sensitive-name policy.
    ///
    /// [`HeaderValue::set_sensitive`] marks the value as
    /// [`qubit_sanitize::SensitivityLevel::Secret`] before name matching.
    /// Header-name exclusions cannot override that value-level declaration;
    /// unmarked values retain the configured name-policy behavior.
    ///
    /// # Parameters
    ///
    /// * `name` - Header name.
    /// * `value` - Header value.
    ///
    /// # Returns
    ///
    /// Secret-masked value for a natively sensitive header; otherwise a value
    /// rendered by the configured name policy.
    #[inline(always)]
    pub(crate) fn header_value(
        &self,
        name: &HeaderName,
        value: &HeaderValue,
    ) -> String {
        self.sanitizer.sanitize_header_value(name, value)
    }

    /// Returns a body preview rendered according to the configured body
    /// sanitization policy.
    ///
    /// Selecting [`qubit_sanitize::TextBodyPolicy::PassThrough`] may expose
    /// secrets from the original opaque body. The returned rendering escapes
    /// log-unsafe characters.
    ///
    /// # Parameters
    ///
    /// * `body` - Raw body bytes.
    /// * `context` - Request/response/error body logging context.
    /// * `content_type` - Optional `Content-Type` header value.
    ///
    /// # Returns
    ///
    /// Human-readable, policy-rendered body preview.
    pub(crate) fn body(
        &self,
        body: &[u8],
        context: BodyLogContext,
        content_type: Option<&HeaderValue>,
    ) -> String {
        let preview = BodyPreview::new(body, self.body_size_limit, context);
        let preview = if let Some(content_type) = content_type {
            preview.with_content_type(content_type)
        } else {
            preview
        };
        self.sanitizer.sanitize_body_preview(&preview)
    }
}
