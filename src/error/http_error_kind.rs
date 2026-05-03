/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! HTTP error category enum.

use parse_display::{
    Display,
    FromStr as DeriveFromStr,
};
use serde::{
    Deserialize,
    Serialize,
};

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
    /// Write timeout.
    WriteTimeout,
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
    /// A single HTTP retry attempt exceeded its configured timeout.
    RetryAttemptTimeout,
    /// Total retry elapsed budget was exceeded before a retryable failure was recorded.
    RetryMaxElapsedExceeded,
    /// Retry stopped because the retry policy decided not to continue (abort).
    RetryAborted,
    /// Any other error.
    Other,
}
