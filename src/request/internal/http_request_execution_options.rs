// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Runtime controls applied to one HTTP request.

use std::time::Duration;

use crate::HttpCancellationToken;
use crate::request::http_request_retry_override::HttpRequestRetryOverride;

/// Per-request runtime controls copied from the builder and used while sending
/// the request and consuming its response.
#[derive(Debug, Clone)]
pub(in crate::request) struct HttpRequestExecutionOptions {
    /// Optional timeout applied to the backend request, overriding the
    /// client-wide request timeout when present.
    pub(in crate::request) request_timeout: Option<Duration>,
    /// Maximum time allowed for request preparation and sending, including
    /// asynchronous header injection and streaming-body preparation.
    pub(in crate::request) response_header_timeout: Duration,
    /// Maximum wait for each backend response-body read.
    pub(in crate::request) read_timeout: Duration,
    /// Optional cooperative cancellation signal checked during request
    /// preparation, sending, and response-body reads.
    pub(in crate::request) cancellation_token: Option<HttpCancellationToken>,
    /// Request-level retry settings that override the client's retry policy
    /// for enablement, eligible methods, and `Retry-After` handling.
    pub(in crate::request) retry_override: HttpRequestRetryOverride,
}
