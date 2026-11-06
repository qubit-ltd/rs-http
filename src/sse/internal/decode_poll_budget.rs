// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Cooperative work budget for one poll of the composed SSE decoder.

use std::task::Poll;

/// Yields after a bounded amount of line/record work or input bytes.
#[derive(Default)]
pub(in crate::sse) struct DecodePollBudget {
    /// Number of lines or complete records consumed since the last yield.
    items: usize,
    /// Number of input bytes consumed since the last yield.
    bytes: usize,
}

impl DecodePollBudget {
    /// Accounts for consumed work and cooperatively yields when a threshold is
    /// reached. Counters restart after the yield so later polls get a fresh
    /// budget.
    ///
    /// # Parameters
    /// - `items`: Number of completed lines or records to account for.
    /// - `bytes`: Number of input bytes consumed by the work.
    pub(in crate::sse) async fn account(&mut self, items: usize, bytes: usize) {
        self.items = self.items.saturating_add(items);
        self.bytes = self.bytes.saturating_add(bytes);
        if self.items >= 256 || self.bytes >= 64 * 1024 {
            self.items = 0;
            self.bytes = 0;
            let mut first_poll = true;
            std::future::poll_fn(|context| {
                if first_poll {
                    first_poll = false;
                    context.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            })
            .await;
            tokio::task::yield_now().await;
        }
    }
}
