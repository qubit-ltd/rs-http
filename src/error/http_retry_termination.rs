// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Terminal conditions reported by an HTTP retry flow.

/// Describes why an HTTP retry flow stopped making attempts.
///
/// The variants distinguish a policy decision, exhausted attempt or duration
/// limits, and cancellation so callers can report the outcome of a retry flow.
///
/// # Examples
///
/// ```
/// use qubit_http::HttpRetryTermination;
///
/// let termination = HttpRetryTermination::AttemptsExhausted;
/// assert_eq!(termination, HttpRetryTermination::AttemptsExhausted);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpRetryTermination {
    /// The retry policy declined another attempt after an attempt completed.
    Aborted,
    /// The retry flow reached its configured maximum number of attempts.
    AttemptsExhausted,
    /// The retry flow used its configured duration budget.
    DurationExceeded,
    /// Cancellation stopped the operation before another attempt could run.
    Cancelled,
}
