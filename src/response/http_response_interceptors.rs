// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Response interceptor abstraction for successful HTTP responses.

use qubit_function::ArcMutatingFunction;
use qubit_function::MutatingFunction;

use super::HttpResponseInterceptorContext;
use super::HttpResponseMeta;
use crate::HttpResult;

/// Response interceptor function used to inspect/mutate response metadata
/// before the response is returned to callers.
///
/// The interceptor receives [`HttpResponseInterceptorContext`], where status
/// and request method are immutable and headers/final URL are mutable.
///
/// Returning `Err` stops the remaining interceptors for the current attempt.
/// The error is enriched with response context when its status, method, or URL
/// is missing.
///
/// # Examples
///
/// ```
/// use qubit_http::HttpResponseInterceptor;
/// use qubit_http::HttpResponseInterceptorContext;
///
/// let interceptor = HttpResponseInterceptor::new(
///     |_context: &mut HttpResponseInterceptorContext| Ok(()),
/// );
/// ```
pub type HttpResponseInterceptor = ArcMutatingFunction<HttpResponseInterceptorContext, HttpResult<()>>;

/// Applies response interceptors in registration order to successful HTTP
/// response metadata.
///
/// Interceptors can update response headers and the final URL through their
/// context. Status and originating request method remain immutable. If any
/// interceptor returns an error, later interceptors do not run, metadata
/// changes are not applied, and the error receives any missing response
/// context.
///
/// # Examples
///
/// ```
/// use qubit_http::HttpResponseInterceptor;
/// use qubit_http::HttpResponseInterceptorContext;
/// use qubit_http::HttpResponseInterceptors;
///
/// let mut interceptors = HttpResponseInterceptors::new();
/// interceptors.push(HttpResponseInterceptor::new(
///     |_context: &mut HttpResponseInterceptorContext| Ok(()),
/// ));
/// ```
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct HttpResponseInterceptors {
    /// Callbacks retained in the order in which they are applied.
    interceptors: Vec<HttpResponseInterceptor>,
}

impl HttpResponseInterceptors {
    /// Creates an empty list ready to receive response interceptors.
    ///
    /// # Returns
    /// An empty interceptor list.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an interceptor that will run after all previously registered
    /// interceptors.
    ///
    /// # Parameters
    /// - `interceptor`: Callback that can inspect and mutate response context.
    pub fn push(&mut self, interceptor: HttpResponseInterceptor) {
        self.interceptors.push(interceptor);
    }

    /// Removes every registered interceptor while retaining the list for
    /// later reuse.
    pub fn clear(&mut self) {
        self.interceptors.clear();
    }

    /// Applies response interceptors in insertion order.
    ///
    /// # Parameters
    /// - `response_meta`: Response metadata to expose and update.
    ///
    /// # Returns
    /// `Ok(())` when all interceptors accept the response.
    ///
    /// # Errors
    /// Returns the first interceptor error and enriches it with
    /// status/method/URL context when missing.
    pub fn apply(&self, response_meta: &mut HttpResponseMeta) -> HttpResult<()> {
        let mut context = HttpResponseInterceptorContext::from_meta(response_meta);
        for interceptor in &self.interceptors {
            interceptor.apply(&mut context).map_err(|error| {
                let mut mapped = error;
                if mapped.status.is_none() {
                    mapped = mapped.with_status(context.status());
                }
                if mapped.method.is_none() {
                    mapped = mapped.with_method(context.method());
                }
                if mapped.url.is_none() {
                    mapped = mapped.with_url(context.url());
                }
                mapped.with_log_redactor(response_meta.log_redactor().clone())
            })?;
        }
        context.apply_to_meta(response_meta);
        Ok(())
    }
}
