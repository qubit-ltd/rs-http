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
/// Returning `Err` short-circuits execution for the current attempt.
pub type HttpResponseInterceptor =
    ArcMutatingFunction<HttpResponseInterceptorContext, HttpResult<()>>;

/// Ordered response interceptor list with unified application behavior.
#[derive(Debug, Clone, Default)]
pub struct HttpResponseInterceptors {
    interceptors: Vec<HttpResponseInterceptor>,
}

impl HttpResponseInterceptors {
    /// Creates an empty response interceptor list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one response interceptor.
    pub fn push(&mut self, interceptor: HttpResponseInterceptor) {
        self.interceptors.push(interceptor);
    }

    /// Removes all response interceptors.
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
