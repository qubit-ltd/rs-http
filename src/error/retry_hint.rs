/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # Retry Hint
//!
//! Provides lightweight retryability classification for HTTP errors.
//!
//! # Author
//!
//! Haixing Hu

/// High-level classification from [`crate::HttpError::retry_hint`] for backoff policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryHint {
    /// Transient failure (timeouts, some 5xx/429, transport); callers may retry with care.
    Retryable,
    /// Permanent or non-idempotent failure; do not retry by default.
    NonRetryable,
}
