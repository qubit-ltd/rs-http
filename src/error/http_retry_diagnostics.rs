// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

pub use super::http_retry_termination::HttpRetryTermination;

/// Stable diagnostics attached to the final HTTP error of a retry flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpRetryDiagnostics {
    attempts: u32,
    elapsed: Duration,
    termination: HttpRetryTermination,
}

impl HttpRetryDiagnostics {
    /// Creates retry diagnostics from the completed flow counters.
    ///
    /// # Parameters
    /// - `attempts`: Number of attempts made by the flow.
    /// - `elapsed`: Total elapsed duration of the flow.
    /// - `termination`: Terminal condition recorded for the flow.
    ///
    /// # Returns
    /// A diagnostics snapshot containing the supplied values.
    pub(crate) const fn new(attempts: u32, elapsed: Duration, termination: HttpRetryTermination) -> Self {
        Self {
            attempts,
            elapsed,
            termination,
        }
    }

    /// Returns the number of attempts made by the retry flow.
    #[must_use]
    pub const fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Returns the elapsed duration of the retry flow.
    #[must_use]
    pub const fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Returns the reason the retry flow terminated.
    #[must_use]
    pub const fn termination(&self) -> HttpRetryTermination {
        self.termination
    }
}
