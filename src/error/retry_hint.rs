// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # Retry Hint
//!
//! Provides lightweight retryability classification for HTTP errors.

use parse_display::{Display, FromStr as DeriveFromStr};
use serde::{Deserialize, Serialize};

/// High-level classification from [`crate::HttpError::retry_hint`] for backoff
/// policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, DeriveFromStr)]
#[serde(rename_all = "snake_case")]
#[display(style = "snake_case")]
pub enum RetryHint {
    /// Transient failure (timeouts, some 5xx/429, transport); callers may
    /// retry with care.
    Retryable,
    /// Permanent or non-idempotent failure; do not retry by default.
    NonRetryable,
}
