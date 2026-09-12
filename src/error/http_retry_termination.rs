// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Terminal conditions reported by an HTTP retry flow.

/// Terminal condition of an HTTP retry flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpRetryTermination {
    /// The retry rule stopped after an attempt.
    Aborted,
    /// The configured attempt limit was reached.
    AttemptsExhausted,
    /// A retry duration budget was reached.
    DurationExceeded,
    /// The operation was cancelled.
    Cancelled,
}
