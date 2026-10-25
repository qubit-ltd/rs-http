// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Outcome of scheduling an SSE reconnect.

use crate::HttpError;

/// Outcome after trying to schedule one reconnect.
#[must_use]
pub(in crate::sse) enum ReconnectAction {
    /// Reconnect sleep completed and caller should start the next attempt.
    Continue,
    /// Reconnect is exhausted without an error to yield.
    Stop,
    /// Reconnect failed and caller should yield this error.
    Fail(Box<HttpError>),
}
