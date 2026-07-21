// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public unified HTTP log redactor.

use http::{
    HeaderMap,
    HeaderValue,
};
use qubit_redact::http::{
    BodyCapture,
    BodyRedaction,
    HttpRedactor,
    RedactedHeaders,
};
use url::Url;

use super::{
    BodyPreview,
    LogRedactionPolicy,
};

/// Delegates every HTTP diagnostic domain to one runtime redactor.
#[must_use = "use the redactor to produce safe HTTP diagnostics"]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRedactor {
    /// Complete application policy snapshot.
    policy: LogRedactionPolicy,
    /// Unified runtime HTTP redactor using the same snapshot.
    http_redactor: HttpRedactor,
}

impl LogRedactor {
    /// Creates a redactor from one immutable log policy.
    ///
    /// # Parameters
    ///
    /// * `policy` - Policy snapshot shared by every rendering method.
    ///
    /// # Returns
    ///
    /// A unified log redactor.
    #[inline]
    pub fn new(policy: LogRedactionPolicy) -> Self {
        let http_redactor = HttpRedactor::new(policy.http_policy().clone());
        Self {
            policy,
            http_redactor,
        }
    }

    /// Returns the immutable application log policy.
    ///
    /// # Returns
    ///
    /// The snapshot shared by every rendering method.
    #[inline(always)]
    pub const fn policy(&self) -> &LogRedactionPolicy {
        &self.policy
    }

    /// Redacts a parsed URL into log-safe text.
    ///
    /// # Parameters
    ///
    /// * `url` - Parsed URL to redact.
    ///
    /// # Returns
    ///
    /// An owned log-safe URL representation.
    #[inline(always)]
    pub fn redact_url(&self, url: &Url) -> qubit_redact::LogSafeText<'static> {
        self.http_redactor.redact_url(url)
    }

    /// Redacts all headers while honoring native sensitive flags.
    ///
    /// # Parameters
    ///
    /// * `headers` - Header map to redact deterministically.
    ///
    /// # Returns
    ///
    /// An opaque rendering that exposes only safe text.
    #[inline(always)]
    pub fn redact_headers(&self, headers: &HeaderMap) -> RedactedHeaders {
        self.http_redactor.redact_headers(headers)
    }

    /// Redacts a body under both presentation and hard runtime limits.
    ///
    /// # Parameters
    ///
    /// * `body` - Complete source body bytes.
    /// * `limit` - Presentation prefix limit, clamped to one byte.
    /// * `content_type` - Optional Content-Type text for parser selection.
    ///
    /// # Returns
    ///
    /// Bounded log-safe text with truthful source and truncation metadata.
    #[inline]
    pub fn redact_body_preview(
        &self,
        body: &[u8],
        limit: usize,
        content_type: Option<&str>,
    ) -> BodyRedaction {
        let preview = BodyPreview::new(body, limit);
        let parsed_content_type = content_type.map(Self::parse_content_type);
        self.http_redactor
            .redact_body(preview.capture(), parsed_content_type.as_ref())
    }

    /// Redacts a body preview using a native Content-Type header value.
    ///
    /// # Parameters
    ///
    /// * `body` - Complete source body bytes.
    /// * `limit` - Presentation prefix limit.
    /// * `content_type` - Optional native Content-Type value.
    ///
    /// # Returns
    ///
    /// A bounded body result with truthful source metadata.
    #[inline(always)]
    pub(crate) fn redact_body_preview_with_header(
        &self,
        body: &[u8],
        limit: usize,
        content_type: Option<&HeaderValue>,
    ) -> BodyRedaction {
        self.http_redactor
            .redact_body(BodyPreview::new(body, limit).capture(), content_type)
    }

    /// Redacts bytes already captured by a streaming caller.
    ///
    /// # Parameters
    ///
    /// * `bytes` - Captured body prefix.
    /// * `source_len` - Exact source length when known.
    /// * `truncated` - Whether capture omitted source bytes.
    /// * `content_type` - Optional native Content-Type value.
    ///
    /// # Returns
    ///
    /// A bounded body result. Invalid exact totals are treated as unknown.
    #[inline]
    pub(crate) fn redact_captured_body(
        &self,
        bytes: &[u8],
        source_len: Option<usize>,
        truncated: bool,
        content_type: Option<&HeaderValue>,
    ) -> BodyRedaction {
        let capture = match (truncated, source_len) {
            (false, _) => BodyCapture::complete(bytes),
            (true, None) => BodyCapture::truncated_unknown(bytes),
            (true, Some(total)) => {
                match BodyCapture::truncated(bytes, Some(total)) {
                    Ok(capture) => capture,
                    Err(_) => BodyCapture::truncated_unknown(bytes),
                }
            }
        };
        self.http_redactor.redact_body(capture, content_type)
    }

    /// Redacts URL-looking tokens in diagnostic text.
    ///
    /// # Parameters
    ///
    /// * `text` - Diagnostic text that may contain absolute HTTP URLs.
    ///
    /// # Returns
    ///
    /// Log-safe text with every recognized URL token redacted under this
    /// snapshot.
    #[inline(always)]
    pub(crate) fn redact_diagnostic_text(
        &self,
        text: &str,
    ) -> qubit_redact::LogSafeText<'static> {
        self.http_redactor.redact_urls_in_text(text)
    }

    /// Parses one Content-Type or returns a deliberately non-UTF-8 value.
    ///
    /// # Parameters
    ///
    /// * `value` - Content-Type text supplied by the public API.
    ///
    /// # Returns
    ///
    /// The parsed value, or a sentinel that makes runtime parsing fail closed.
    fn parse_content_type(value: &str) -> HeaderValue {
        match HeaderValue::from_str(value) {
            Ok(value) => value,
            Err(_) => HeaderValue::from_bytes(&[0xff])
                .expect("fixed non-UTF-8 header sentinel must be valid"),
        }
    }
}

impl Default for LogRedactor {
    /// Creates a redactor from [`LogRedactionPolicy::default`].
    #[inline(always)]
    fn default() -> Self {
        Self::new(LogRedactionPolicy::default())
    }
}
