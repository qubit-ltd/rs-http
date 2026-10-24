// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Borrowed request and policy context shared during reconnect scheduling.

use http::Method;
use qubit_redact::Redactor;
use qubit_retry::RetryPolicy;
use url::Url;

use crate::HttpCancellationToken;

/// Runtime references shared by one reconnect scheduling decision.
pub(super) struct ReconnectRuntime<'a> {
    /// Retry policy used to derive delay and jitter behavior.
    pub(super) retry_policy: &'a RetryPolicy,
    /// SSE reconnect options controlling server retry and EOF behavior.
    pub(super) options: &'a super::super::SseReconnectOptions,
    /// Optional cancellation token checked while sleeping before reconnect.
    pub(super) cancellation_token: Option<&'a HttpCancellationToken>,
    /// Request method used in reconnect cancellation and max-elapsed errors.
    pub(super) request_method: &'a Method,
    /// Request URL used in reconnect cancellation and max-elapsed errors.
    pub(super) request_url: Option<&'a Url>,
    /// Shared request redactor used by reconnect-generated errors.
    pub(super) log_redactor: &'a Redactor,
}
