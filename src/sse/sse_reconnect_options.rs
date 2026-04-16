/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Options and defaults for SSE reconnect with backoff.
//!
//! # Author
//!
//! Haixing Hu

use std::time::Duration;

use crate::{RetryDelay, RetryJitter, RetryOptions};

/// Default upper bound for SSE reconnect delay backoff.
pub(crate) const DEFAULT_SSE_MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);

/// Default exponential backoff multiplier for SSE reconnect delay growth.
pub(crate) const DEFAULT_SSE_RECONNECT_BACKOFF_MULTIPLIER: f64 = 2.0;

/// Default maximum reconnect attempts after the initial stream open.
pub(crate) const DEFAULT_SSE_MAX_RECONNECTS: u32 = 3;

/// Builds default retry options for SSE reconnect.
///
/// # Returns
/// Retry options matching SSE reconnect defaults.
fn default_sse_retry_options() -> RetryOptions {
    RetryOptions::new(
        DEFAULT_SSE_MAX_RECONNECTS + 1,
        None,
        RetryDelay::exponential(
            Duration::from_secs(1),
            DEFAULT_SSE_MAX_RECONNECT_DELAY,
            DEFAULT_SSE_RECONNECT_BACKOFF_MULTIPLIER,
        ),
        RetryJitter::None,
    )
    .expect("SSE default retry options must be valid")
}

/// Reconnect behavior options for [`crate::HttpClient::execute_sse_with_reconnect`].
#[derive(Debug, Clone, PartialEq)]
pub struct SseReconnectOptions {
    /// Retry options used by SSE reconnect delay calculation.
    ///
    /// `max_attempts` includes the initial stream-open attempt, so if callers
    /// want at most `N` reconnects they should pass `max_attempts = N + 1`.
    pub retry: RetryOptions,
    /// Whether to reconnect when the SSE stream ends without an explicit error.
    pub reconnect_on_eof: bool,
    /// Whether to honor SSE `retry:` field as the next reconnect delay.
    pub honor_server_retry: bool,
}

impl Default for SseReconnectOptions {
    /// Builds default SSE reconnect options.
    ///
    /// # Returns
    /// Default reconnect options with bounded reconnect attempts.
    fn default() -> Self {
        Self {
            retry: default_sse_retry_options(),
            reconnect_on_eof: true,
            honor_server_retry: true,
        }
    }
}

impl SseReconnectOptions {
    /// Creates default SSE reconnect options.
    ///
    /// # Returns
    /// Same as [`SseReconnectOptions::default`].
    pub fn new() -> Self {
        Self::default()
    }
}
