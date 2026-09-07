// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
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
    pub(crate) const fn new(attempts: u32, elapsed: Duration, termination: HttpRetryTermination) -> Self {
        Self {
            attempts,
            elapsed,
            termination,
        }
    }

    #[must_use]
    pub const fn attempts(&self) -> u32 {
        self.attempts
    }

    #[must_use]
    pub const fn elapsed(&self) -> Duration {
        self.elapsed
    }

    #[must_use]
    pub const fn termination(&self) -> HttpRetryTermination {
        self.termination
    }
}
