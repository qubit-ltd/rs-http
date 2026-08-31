// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
//! Safe rendering helpers for HTTP TRACE logs.

use http::HeaderValue;
use qubit_redact::RedactionTextOutput;
use qubit_redact::Redactor;

use super::BodyPreview;
use crate::HttpClientOptions;

/// Applies one policy snapshot and one presentation body limit to TRACE data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RedactedLogger {
    /// Unified redactor for URLs, headers, and bodies.
    redactor: Redactor,
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
    pub(crate) fn new(log_redactor: Redactor, body_size_limit: usize) -> Self {
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
    pub(crate) fn from_options_with_redactor(options: &HttpClientOptions, log_redactor: Redactor) -> Self {
        Self::new(log_redactor, options.logging.body_size_limit)
    }

    /// Creates one diagnostic session for a complete TRACE record.
    #[inline(always)]
    pub(crate) const fn redactor(&self) -> &Redactor {
        &self.redactor
    }

    /// Redacts a body preview through a shared diagnostic session.
    ///
    /// # Parameters
    ///
    /// * `body` - Complete source bytes offered by the logging layer.
    /// * `content_type` - Optional native Content-Type used for parser choice.
    /// * `session` - Shared diagnostic session for the enclosing TRACE record.
    ///
    /// # Returns
    ///
    /// A structured body result whose status remains independent of
    /// completion. `Complete` carries the full log-safe representation,
    /// `Truncated` carries a non-empty safe substitute, and `Exhausted` carries
    /// empty safe text after terminal output-budget exhaustion. In the
    /// exhausted state the shared session has stopped processing and this
    /// adapter does not read further body bytes.
    #[inline]
    pub(crate) fn body(&self, body: &[u8], content_type: Option<&HeaderValue>) -> RedactionTextOutput {
        self.redactor
            .redact_http_body(BodyPreview::new(body, self.body_size_limit).capture(), content_type)
    }
}
