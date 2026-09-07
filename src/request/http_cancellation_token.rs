// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use std::future::Future;

/// Cancellation token owned by the HTTP API.
#[derive(Clone, Debug, Default)]
pub struct HttpCancellationToken {
    inner: qubit_retry::RetryCancellationToken,
}

impl HttpCancellationToken {
    #[must_use = "await the cancellation future"]
    pub fn new() -> Self {
        Self {
            inner: qubit_retry::RetryCancellationToken::new(),
        }
    }

    pub fn cancel(&self) {
        self.inner.cancel();
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    pub fn cancelled(&self) -> impl Future<Output = ()> + '_ {
        self.inner.cancelled()
    }

    pub(crate) fn inner(&self) -> &qubit_retry::RetryCancellationToken {
        &self.inner
    }

    pub(crate) fn shares_source_with(&self, other: &Self) -> bool {
        self.inner.shares_source_with(&other.inner)
    }
}
