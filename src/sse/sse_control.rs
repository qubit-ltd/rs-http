// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// SSE control record that affects stream state but is not a user message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SseControl {
    /// Reconnection delay from a valid `retry:` field, in milliseconds.
    ReconnectDelayMs(
        /// Delay from the SSE `retry:` value.
        u64,
    ),
    /// Last event id update from an `id:` field without dispatchable data.
    LastEventId(
        /// Event ID carried by the control record.
        String,
    ),
}
