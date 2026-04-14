/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Async header injector abstraction for outgoing requests.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use http::HeaderMap;

use crate::HttpResult;

type AsyncHeaderInjectorFuture<'a> = Pin<Box<dyn Future<Output = HttpResult<()>> + Send + 'a>>;
type AsyncHeaderInjectorFn =
    dyn for<'a> Fn(&'a mut HeaderMap) -> AsyncHeaderInjectorFuture<'a> + Send + Sync + 'static;

/// Async header injector that can await external state (for example token refresh)
/// before mutating outbound request headers.
#[derive(Clone)]
pub struct AsyncHeaderInjector {
    /// Underlying async mutation callback.
    inner: Arc<AsyncHeaderInjectorFn>,
}

impl std::fmt::Debug for AsyncHeaderInjector {
    /// Formats this injector without exposing closure internals.
    ///
    /// # Parameters
    /// - `f`: Formatter destination.
    ///
    /// # Returns
    /// Formatting result.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncHeaderInjector")
            .finish_non_exhaustive()
    }
}

impl AsyncHeaderInjector {
    /// Creates an async header injector from a callback.
    ///
    /// # Parameters
    /// - `injector`: Async callback that mutates `HeaderMap` and may fail.
    ///
    /// # Returns
    /// New [`AsyncHeaderInjector`].
    pub fn new<F>(injector: F) -> Self
    where
        F: for<'a> Fn(&'a mut HeaderMap) -> AsyncHeaderInjectorFuture<'a> + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(injector),
        }
    }

    /// Applies this injector to `headers`.
    ///
    /// # Parameters
    /// - `headers`: Header map to mutate.
    ///
    /// # Returns
    /// `Ok(())` on success.
    ///
    /// # Errors
    /// Propagates callback-provided [`crate::HttpError`].
    pub async fn apply(&self, headers: &mut HeaderMap) -> HttpResult<()> {
        (self.inner)(headers).await
    }
}
