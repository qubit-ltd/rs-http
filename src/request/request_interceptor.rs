/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Request interceptor abstraction for outgoing HTTP requests.

use qubit_function::{ArcMutatingFunction, MutatingFunction};
use url::Url;

use super::http_request::HttpRequest;
use crate::HttpResult;

/// Request interceptor function used to mutate an outbound [`HttpRequest`]
/// before URL resolution, header merge, and network I/O.
///
/// Returning `Err` short-circuits execution for the current attempt.
pub type HttpRequestInterceptor = ArcMutatingFunction<HttpRequest, HttpResult<()>>;

/// Ordered request interceptor list with unified application behavior.
#[derive(Debug, Clone, Default)]
pub struct HttpRequestInterceptors {
    interceptors: Vec<HttpRequestInterceptor>,
}

impl HttpRequestInterceptors {
    /// Creates an empty request interceptor list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one request interceptor.
    pub fn push(&mut self, interceptor: HttpRequestInterceptor) {
        self.interceptors.push(interceptor);
    }

    /// Removes all request interceptors.
    pub fn clear(&mut self) {
        self.interceptors.clear();
    }

    /// Applies request interceptors in insertion order.
    ///
    /// # Parameters
    /// - `request`: Request snapshot to mutate before URL resolution and send.
    ///
    /// # Returns
    /// `Ok(())` when all interceptors succeed.
    ///
    /// # Errors
    /// Returns the first interceptor error and enriches it with method/URL
    /// context when missing.
    pub fn apply(&self, request: &mut HttpRequest) -> HttpResult<()> {
        for interceptor in &self.interceptors {
            interceptor.apply(request).map_err(|error| {
                let mut mapped = error;
                if mapped.method.is_none() {
                    mapped = mapped.with_method(request.method());
                }
                if mapped.url.is_none() {
                    // Prefer the cached resolved URL so relative paths carry full
                    // base_url context in interceptor errors; fallback to parsing
                    // raw path to preserve behavior when cache is unavailable.
                    if let Some(resolved_url) = request.resolved_url_cached() {
                        mapped = mapped.with_url(&resolved_url);
                    } else if let Ok(parsed_url) = Url::parse(request.path()) {
                        mapped = mapped.with_url(&parsed_url);
                    }
                }
                mapped
            })?;
        }
        Ok(())
    }
}
