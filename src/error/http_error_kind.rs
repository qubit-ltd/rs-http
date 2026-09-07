// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! HTTP error category enum.

use parse_display::Display;
use parse_display::FromStr as DeriveFromStr;
use serde::Deserialize;
use serde::Serialize;

/// Category of HTTP errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, DeriveFromStr)]
#[serde(rename_all = "snake_case")]
#[display(style = "snake_case")]
pub enum HttpErrorKind {
    /// URL is invalid or cannot be resolved.
    InvalidUrl,
    /// HTTP client construction failed.
    BuildClient,
    /// Proxy configuration is invalid.
    ProxyConfig,
    /// Connect timeout.
    ConnectTimeout,
    /// Read timeout.
    ReadTimeout,
    /// Request preparation and header-response timeout.
    SendTimeout,
    /// Whole-request timeout (client/request-level deadline).
    RequestTimeout,
    /// Transport-level request error.
    Transport,
    /// Non-success HTTP status.
    Status,
    /// Response decoding error.
    Decode,
    /// SSE protocol error.
    SseProtocol,
    /// SSE payload decoding error.
    SseDecode,
    /// Request was cancelled or interrupted.
    Cancelled,
    /// Retry budget was exceeded before an application error was captured.
    RetryBudgetExceeded,
    /// Aggregating the response body exceeded its configured limit.
    ResponseBodyTooLarge,
    /// The response body stream has already been taken.
    ResponseBodyAlreadyConsumed,
    /// The request target violates the configured origin policy.
    OriginPolicy,
    /// Any other error.
    Other,
}
