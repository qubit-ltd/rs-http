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

use crate::RetryJitter;

/// Default upper bound for SSE reconnect delay backoff.
pub(crate) const DEFAULT_SSE_MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);

/// Default exponential backoff multiplier for SSE reconnect delay growth.
pub(crate) const DEFAULT_SSE_RECONNECT_BACKOFF_MULTIPLIER: f64 = 2.0;

/// Reconnect behavior options for [`crate::HttpClient::execute_sse_with_reconnect`].
#[derive(Debug, Clone, PartialEq)]
pub struct SseReconnectOptions {
    /// Maximum reconnect attempts after the first connection.
    pub max_reconnects: u32,
    /// Base reconnect delay between attempts.
    pub reconnect_delay: Duration,
    /// Upper bound for exponential reconnect backoff delay.
    pub max_reconnect_delay: Duration,
    /// Exponential backoff multiplier applied after each reconnect sleep.
    pub reconnect_backoff_multiplier: f64,
    /// Retry jitter strategy applied to each reconnect delay.
    pub reconnect_jitter: RetryJitter,
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
            max_reconnects: 3,
            reconnect_delay: Duration::from_secs(1),
            max_reconnect_delay: DEFAULT_SSE_MAX_RECONNECT_DELAY,
            reconnect_backoff_multiplier: DEFAULT_SSE_RECONNECT_BACKOFF_MULTIPLIER,
            reconnect_jitter: RetryJitter::None,
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
