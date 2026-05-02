/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Response interceptor abstraction for successful HTTP responses.

use qubit_function::{ArcMutatingFunction, MutatingFunction};

use super::HttpResponseMeta;
use crate::HttpResult;

/// Response interceptor function used to inspect/mutate response metadata
/// (`status`, `headers`, `url`) before the response is returned to callers.
///
/// Returning `Err` short-circuits execution for the current attempt.
pub type HttpResponseInterceptor = ArcMutatingFunction<HttpResponseMeta, HttpResult<()>>;

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
    /// - `response_meta`: Response metadata.
    ///
    /// # Returns
    /// `Ok(())` when all interceptors accept the response.
    ///
    /// # Errors
    /// Returns the first interceptor error and enriches it with
    /// status/method/URL context when missing.
    pub fn apply(&self, response_meta: &mut HttpResponseMeta) -> HttpResult<()> {
        for interceptor in &self.interceptors {
            interceptor.apply(response_meta).map_err(|error| {
                let mut mapped = error;
                if mapped.status.is_none() {
                    mapped = mapped.with_status(response_meta.status);
                }
                if mapped.method.is_none() {
                    mapped = mapped.with_method(&response_meta.method);
                }
                if mapped.url.is_none() {
                    mapped = mapped.with_url(&response_meta.url);
                }
                mapped
            })?;
        }
        Ok(())
    }
}
