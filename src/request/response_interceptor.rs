/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Response interceptor abstraction for successful HTTP responses.

use std::sync::Arc;

use http::{HeaderMap, Method, StatusCode};
use url::Url;

use crate::HttpResult;

type ResponseInterceptorFn =
    dyn Fn(StatusCode, &HeaderMap, &Method, &Url) -> HttpResult<()> + Send + Sync + 'static;

/// Response interceptor called after success-status validation and before the
/// response is returned to the caller.
///
/// Returning `Err` short-circuits the current attempt.
#[derive(Clone)]
pub struct ResponseInterceptor {
    inner: Arc<ResponseInterceptorFn>,
}

impl std::fmt::Debug for ResponseInterceptor {
    /// Formats this interceptor without exposing closure internals.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponseInterceptor")
            .finish_non_exhaustive()
    }
}

impl ResponseInterceptor {
    /// Creates a response interceptor callback.
    ///
    /// # Parameters
    /// - `interceptor`: Callback receiving status, headers, method, and URL.
    ///
    /// # Returns
    /// New [`ResponseInterceptor`].
    pub fn new<F>(interceptor: F) -> Self
    where
        F: Fn(StatusCode, &HeaderMap, &Method, &Url) -> HttpResult<()> + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(interceptor),
        }
    }

    /// Applies this interceptor.
    ///
    /// # Parameters
    /// - `status`: Response status code.
    /// - `headers`: Response headers.
    /// - `method`: Request method.
    /// - `url`: Request URL.
    ///
    /// # Returns
    /// `Ok(())` when accepted by interceptor.
    pub fn apply(
        &self,
        status: StatusCode,
        headers: &HeaderMap,
        method: &Method,
        url: &Url,
    ) -> HttpResult<()> {
        (self.inner)(status, headers, method, url)
    }
}
