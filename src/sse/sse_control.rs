/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

/// SSE control record that affects stream state but is not a user message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SseControl {
    /// Reconnection delay from a valid `retry:` field, in milliseconds.
    ReconnectDelayMs(u64),
    /// Last event id update from an `id:` field without dispatchable data.
    LastEventId(String),
}
