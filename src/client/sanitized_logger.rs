/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Sanitized rendering helpers for HTTP TRACE logs.

use http::{
    HeaderName,
    HeaderValue,
};
use url::Url;

use crate::{
    BodyLogContext,
    BodyPreview,
    HttpClientOptions,
    LogSanitizePolicy,
    LogSanitizer,
};

/// Renders URL, header, and body values for safe HTTP logs.
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
    /// - `policy`: Sanitization policy used for URL/header/body values.
    /// - `body_size_limit`: Maximum bytes rendered from body previews.
    ///
    /// # Returns
    /// New sanitized logger helper.
    pub(crate) fn new(policy: LogSanitizePolicy, body_size_limit: usize) -> Self {
        Self {
            sanitizer: LogSanitizer::new(policy),
            body_size_limit,
        }
    }

    /// Creates a sanitized logger from client options.
    ///
    /// # Parameters
    /// - `options`: Client options carrying logging and sanitization policy.
    ///
    /// # Returns
    /// Sanitized logger configured like [`crate::HttpLogger`].
    pub(crate) fn from_options(options: &HttpClientOptions) -> Self {
        Self::new(
            options.log_sanitize_policy.clone(),
            options.logging.body_size_limit,
        )
    }

    /// Returns a log-safe URL string.
    ///
    /// # Parameters
    /// - `url`: URL to render.
    ///
    /// # Returns
    /// URL with userinfo, fragment, and sensitive query values masked.
    pub(crate) fn url(&self, url: &Url) -> String {
        self.sanitizer.sanitize_url(url)
    }

    /// Returns a log-safe header value.
    ///
    /// # Parameters
    /// - `name`: Header name.
    /// - `value`: Header value.
    ///
    /// # Returns
    /// Masked header value when the name is sensitive, otherwise UTF-8 text or
    /// `<non-utf8>`.
    pub(crate) fn header_value(&self, name: &HeaderName, value: &HeaderValue) -> String {
        self.sanitizer.sanitize_header_value(name, value)
    }

    /// Returns a log-safe body preview.
    ///
    /// # Parameters
    /// - `body`: Raw body bytes.
    /// - `context`: Request/response/error body logging context.
    /// - `content_type`: Optional `Content-Type` header value.
    ///
    /// # Returns
    /// Human-readable sanitized body preview.
    pub(crate) fn body(
        &self,
        body: &[u8],
        context: BodyLogContext,
        content_type: Option<&str>,
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
