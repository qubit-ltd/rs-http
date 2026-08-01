// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
//! Safe rendering helpers for HTTP TRACE logs.

use std::sync::Arc;

use http::{
    HeaderMap,
    HeaderValue,
};
use qubit_redact::http::{
    HttpRedactor,
    RedactedHeaders,
};
use url::Url;

use crate::HttpClientOptions;

use super::BodyPreview;

/// Applies one policy snapshot and one presentation body limit to TRACE data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RedactedLogger {
    /// Unified redactor for URLs, headers, and bodies.
    redactor: Arc<HttpRedactor>,
    /// Maximum source bytes offered by the presentation layer.
    body_size_limit: usize,
}

impl RedactedLogger {
    /// Creates a logger from an explicit policy and presentation limit.
    ///
    /// # Parameters
    ///
    /// * `policy` - Exact immutable policy snapshot.
    /// * `body_size_limit` - Presentation prefix limit for body logs.
    ///
    /// # Returns
    ///
    /// A safe TRACE rendering helper.
    #[inline]
    pub(crate) fn new(
        log_redactor: Arc<HttpRedactor>,
        body_size_limit: usize,
    ) -> Self {
        Self {
            redactor: log_redactor,
            body_size_limit,
        }
    }

    /// Creates a logger from a client option snapshot and its shared redactor.
    ///
    /// # Parameters
    ///
    /// * `options` - Client options carrying policy and presentation limits.
    /// * `log_redactor` - Exact shared redactor owned by the client lifecycle.
    ///
    /// # Returns
    ///
    /// A helper using the supplied immutable redactor snapshot.
    #[inline(always)]
    pub(crate) fn from_options_with_redactor(
        options: &HttpClientOptions,
        log_redactor: Arc<HttpRedactor>,
    ) -> Self {
        Self::new(log_redactor, options.logging.body_size_limit)
    }

    /// Returns a redacted URL representation.
    ///
    /// # Parameters
    ///
    /// * `url` - Parsed URL to redact.
    ///
    /// # Returns
    ///
    /// An owned log-safe URL.
    #[inline(always)]
    pub(crate) fn url(&self, url: &Url) -> qubit_redact::LogSafeText<'static> {
        self.redactor.redact_url(url)
    }

    /// Returns an opaque safe rendering of all headers.
    ///
    /// # Parameters
    ///
    /// * `headers` - Header map to redact.
    ///
    /// # Returns
    ///
    /// A deterministic safe header rendering.
    #[inline(always)]
    pub(crate) fn headers(&self, headers: &HeaderMap) -> RedactedHeaders {
        self.redactor.redact_headers(headers)
    }

    /// Returns a bounded, redacted body preview.
    ///
    /// # Parameters
    ///
    /// * `body` - Complete body bytes.
    /// * `content_type` - Optional native Content-Type value.
    ///
    /// # Returns
    ///
    /// `<empty>` for an empty body, otherwise bounded log-safe text.
    #[inline]
    pub(crate) fn body(
        &self,
        body: &[u8],
        content_type: Option<&HeaderValue>,
    ) -> String {
        if body.is_empty() {
            return "<empty>".to_owned();
        }
        self.redactor
            .redact_body(
                BodyPreview::new(body, self.body_size_limit).capture(),
                content_type,
            )
            .into_log_safe_text()
            .into_owned()
    }
}
