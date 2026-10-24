// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::future::Future;

use qubit_retry::RetryCancellationToken;

/// Cancellation token owned by the HTTP API.
///
/// # Examples
///
/// ```
/// use qubit_http::HttpCancellationToken;
///
/// let token = HttpCancellationToken::new();
/// assert!(!token.is_cancelled());
/// token.cancel();
/// assert!(token.is_cancelled());
/// ```
#[derive(Clone, Debug, Default)]
pub struct HttpCancellationToken {
    inner: RetryCancellationToken,
}

impl HttpCancellationToken {
    /// Creates a token in the not-cancelled state.
    #[must_use = "retain the token to coordinate cancellation"]
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: RetryCancellationToken::new(),
        }
    }

    /// Marks this token as cancelled and wakes waiters.
    #[inline]
    pub fn cancel(&self) {
        self.inner.cancel();
    }

    /// Returns whether cancellation has been requested.
    #[must_use]
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    /// Returns a future that completes when this token is cancelled.
    #[inline]
    pub fn cancelled(&self) -> impl Future<Output = ()> + '_ {
        self.inner.cancelled()
    }

    /// Returns the underlying retry cancellation token for crate-internal use.
    #[must_use]
    #[inline]
    pub(crate) fn inner(&self) -> &RetryCancellationToken {
        &self.inner
    }

    /// Returns whether two HTTP tokens share the same cancellation source.
    ///
    /// # Parameters
    /// - `other`: Token to compare with this token.
    ///
    /// # Returns
    /// `true` when both tokens cancel the same underlying source.
    #[must_use]
    #[inline]
    pub(crate) fn shares_source_with(&self, other: &Self) -> bool {
        self.inner.shares_source_with(&other.inner)
    }
}
