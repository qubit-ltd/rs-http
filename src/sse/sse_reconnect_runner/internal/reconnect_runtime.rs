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
pub(in crate::sse) struct ReconnectRuntime<'a> {
    /// Retry policy used to derive delay and jitter behavior.
    pub(in crate::sse) retry_policy: &'a RetryPolicy,
    /// SSE reconnect options controlling server retry and EOF behavior.
    pub(in crate::sse) options: &'a super::super::SseReconnectOptions,
    /// Optional cancellation token checked while sleeping before reconnect.
    pub(in crate::sse) cancellation_token: Option<&'a HttpCancellationToken>,
    /// Request method used in reconnect cancellation and max-elapsed errors.
    pub(in crate::sse) request_method: &'a Method,
    /// Request URL used in reconnect cancellation and max-elapsed errors.
    pub(in crate::sse) request_url: Option<&'a Url>,
    /// Shared request redactor used by reconnect-generated errors.
    pub(in crate::sse) log_redactor: &'a Redactor,
}
