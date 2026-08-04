// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
//! Safe rendering helpers for HTTP TRACE logs.

use http::{
    HeaderMap,
    HeaderValue,
};
use qubit_redact::http::{
    HttpRedactor,
    RedactedHeaders,
};
use qubit_redact::RedactionSession;
use url::Url;

use crate::HttpClientOptions;

use super::BodyPreview;

/// Applies one policy snapshot and one presentation body limit to TRACE data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RedactedLogger {
    /// Unified redactor for URLs, headers, and bodies.
    redactor: HttpRedactor,
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
        log_redactor: HttpRedactor,
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
        log_redactor: HttpRedactor,
    ) -> Self {
        Self::new(log_redactor, options.logging.body_size_limit)
    }

    /// Creates one diagnostic session for a complete TRACE record.
    #[inline(always)]
    pub(crate) fn session(&self) -> RedactionSession<'_> {
        RedactionSession::diagnostic(self.redactor.policy())
    }

    /// Returns a redacted URL through a shared diagnostic session.
    #[inline(always)]
    pub(crate) fn url_with_session(
        &self,
        url: &Url,
        session: &RedactionSession<'_>,
    ) -> qubit_redact::LogSafeText<'static> {
        self.redactor.redact_url_with_session(url, session)
    }

    /// Returns redacted headers through a shared diagnostic session.
    #[inline(always)]
    pub(crate) fn headers_with_session(
        &self,
        headers: &HeaderMap,
        session: &RedactionSession<'_>,
    ) -> RedactedHeaders {
        self.redactor.redact_headers_with_session(headers, session)
    }

    /// Returns a redacted body preview through a shared diagnostic session.
    #[inline]
    pub(crate) fn body_with_session(
        &self,
        body: &[u8],
        content_type: Option<&HeaderValue>,
        session: &RedactionSession<'_>,
    ) -> String {
        if body.is_empty() {
            return "<empty>".to_owned();
        }
        self.redactor
            .redact_body_with_session(
                BodyPreview::new(body, self.body_size_limit).capture(),
                content_type,
                session,
            )
            .into_log_safe_text()
            .into_owned()
    }
}
