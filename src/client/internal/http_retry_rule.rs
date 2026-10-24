// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Retry-rule state used by the HTTP client's retry adapter.

use crate::HttpRetryOptions;

/// HTTP-specific retry decisions for one request execution.
///
/// The retry adapter consults the effective options after each failed attempt
/// and may use the request's `Retry-After` preference to override its delay.
pub(in crate::client) struct HttpRetryRule {
    /// Fully resolved retry policy used to classify HTTP failures and select
    /// the retry schedule for this request.
    pub(in crate::client) options: HttpRetryOptions,

    /// Whether the request permits a server-provided `Retry-After` delay to
    /// replace the policy-selected delay after a retryable failure.
    pub(in crate::client) honor_retry_after: bool,
}
