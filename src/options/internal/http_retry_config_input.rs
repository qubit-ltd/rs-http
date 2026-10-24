// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Raw retry values read before defaults and validation are applied.

use std::time::Duration;

/// Retry configuration values before conversion into validated options.
pub(in crate::options) struct HttpRetryConfigInput {
    /// Optional switch controlling whether built-in retries are enabled.
    pub(in crate::options) enabled: Option<bool>,
    /// Optional maximum number of attempts, including the initial request.
    pub(in crate::options) max_attempts: Option<u32>,
    /// Optional soft time budget for admitting retry attempts.
    pub(in crate::options) max_duration: Option<Duration>,
    /// Optional name of the retry delay strategy.
    pub(in crate::options) delay_strategy: Option<String>,
    /// Optional delay used by the fixed strategy.
    pub(in crate::options) fixed_delay: Option<Duration>,
    /// Optional lower bound used by the random strategy.
    pub(in crate::options) random_min_delay: Option<Duration>,
    /// Optional upper bound used by the random strategy.
    pub(in crate::options) random_max_delay: Option<Duration>,
    /// Optional initial delay used by the exponential strategy.
    pub(in crate::options) backoff_initial_delay: Option<Duration>,
    /// Optional maximum delay used by the exponential strategy.
    pub(in crate::options) backoff_max_delay: Option<Duration>,
    /// Optional multiplier used by the exponential strategy.
    pub(in crate::options) backoff_multiplier: Option<f64>,
    /// Optional bounded jitter factor applied to the selected strategy.
    pub(in crate::options) jitter_factor: Option<f64>,
    /// Optional method replay policy name.
    pub(in crate::options) method_policy: Option<String>,
    /// Optional list of retryable HTTP status-code strings.
    pub(in crate::options) status_codes: Option<Vec<String>>,
    /// Optional list of retryable non-status error-kind names.
    pub(in crate::options) error_kinds: Option<Vec<String>>,
}
