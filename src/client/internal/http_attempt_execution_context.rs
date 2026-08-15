// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cancellation ownership for one HTTP attempt invocation.

use crate::HttpRequest;
use crate::RetryCancellationToken;

/// Per-call cancellation routing that never escapes into a request clone.
#[derive(Clone, Debug, Default)]
pub(in crate::client) struct HttpAttemptExecutionContext {
    /// Original token source exclusively owned by `AsyncRetry`.
    retry_flow_token: Option<RetryCancellationToken>,
}

impl HttpAttemptExecutionContext {
    /// Creates direct single-attempt routing where request I/O owns its token.
    ///
    /// # Returns
    /// A context without a retry-flow token, so request I/O observes the
    /// request's current cancellation token directly.
    pub(in crate::client) fn direct() -> Self {
        Self::default()
    }

    /// Creates retry routing that captures the original flow token source.
    ///
    /// # Parameters
    /// - `request`: Request before attempt-local interceptors mutate it.
    ///
    /// # Returns
    /// A context holding a clone of the request's original cancellation token,
    /// or no flow token when the request is not cancellable.
    pub(in crate::client) fn retry(request: &HttpRequest) -> Self {
        Self {
            retry_flow_token: request.cancellation_token().cloned(),
        }
    }

    /// Returns whether `AsyncRetry` still owns the request's current token.
    ///
    /// Setting another clone of the original token keeps the same cancellation
    /// source under the retry controller. Only a genuinely independent source
    /// becomes attempt I/O's responsibility.
    ///
    /// # Parameters
    /// - `request`: Request after attempt-local interceptors have run.
    ///
    /// # Returns
    /// `true` when the request still carries the retry-flow cancellation
    /// source; `false` when it was cleared, replaced, or never existed.
    fn retry_flow_owns_current_token(&self, request: &HttpRequest) -> bool {
        let Some(flow_token) = self.retry_flow_token.as_ref() else {
            return false;
        };
        let Some(current_token) = request.cancellation_token() else {
            return false;
        };
        current_token.shares_source_with(flow_token)
    }

    /// Returns whether successful response handoff should restore the flow
    /// token.
    ///
    /// # Parameters
    /// - `request`: Request after attempt-local interceptors have run.
    ///
    /// # Returns
    /// `true` when the successful response must inherit the retry-flow token;
    /// otherwise `false`.
    pub(in crate::client) fn should_restore_retry_flow_token(
        &self,
        request: &HttpRequest,
    ) -> bool {
        self.retry_flow_owns_current_token(request)
    }

    /// Selects the token owned by request I/O for this attempt.
    ///
    /// The retry-flow source is excluded because `AsyncRetry` owns its terminal
    /// classification. A genuinely independent interceptor replacement is
    /// effective for attempt I/O and response reads.
    ///
    /// # Parameters
    /// - `request`: Request after attempt-local interceptors have run.
    ///
    /// # Returns
    /// `Some` for a direct request token or an independent interceptor
    /// replacement. Returns `None` when the token is absent or belongs to the
    /// retry flow and is therefore owned by `AsyncRetry`.
    pub(in crate::client) fn io_cancellation_token<'a>(
        &self,
        request: &'a HttpRequest,
    ) -> Option<&'a RetryCancellationToken> {
        if self.retry_flow_owns_current_token(request) {
            None
        } else {
            request.cancellation_token()
        }
    }
}
