// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair

use super::SseControl;
use super::SseMessage;

/// Internal SSE record stream item before public message filtering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SseRecord {
    /// Dispatchable EventSource message.
    Dispatch(SseMessage),
    /// Control state that should not be exposed as a message.
    Control(SseControl),
}
