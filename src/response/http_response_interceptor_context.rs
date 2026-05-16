/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Response interceptor context with controlled metadata mutation.

use std::fmt;
use std::time::Duration;

use http::{
    HeaderMap,
    Method,
    StatusCode,
};
use url::Url;

use super::HttpResponseMeta;
use crate::LogSanitizePolicy;

/// Metadata view passed to response interceptors.
///
/// The HTTP status and originating method are immutable so interceptors cannot
/// invalidate the `HttpClient::execute` success-status contract after the
/// client has already accepted the response. Interceptors may still mutate
/// response headers and the final response URL.
#[derive(Clone)]
pub struct HttpResponseInterceptorContext {
    /// Response status code captured before interceptor execution.
    status: StatusCode,
    /// Mutable response headers visible to later interceptors and callers.
    headers: HeaderMap,
    /// Mutable final response URL visible to later interceptors and callers.
    url: Url,
    /// Originating request method captured before interceptor execution.
    method: Method,
    /// Sanitization policy snapshot used by standalone debug output.
    log_sanitize_policy: LogSanitizePolicy,
}

impl HttpResponseInterceptorContext {
    /// Creates a response interceptor context from explicit metadata parts.
    ///
    /// # Parameters
    /// - `status`: Response status code. It is immutable after construction.
    /// - `headers`: Response headers that interceptors may mutate.
    /// - `url`: Final response URL that interceptors may replace.
    /// - `method`: Originating request method. It is immutable after construction.
    ///
    /// # Returns
    /// New interceptor context.
    pub fn new(status: StatusCode, headers: HeaderMap, url: Url, method: Method) -> Self {
        Self {
            status,
            headers,
            url,
            method,
            log_sanitize_policy: LogSanitizePolicy::default(),
        }
    }

    /// Attaches the log sanitization policy used for standalone debug output.
    ///
    /// # Parameters
    /// - `policy`: Policy snapshot to apply when formatting this context.
    ///
    /// # Returns
    /// Updated context.
    pub fn with_log_sanitize_policy(mut self, policy: LogSanitizePolicy) -> Self {
        self.log_sanitize_policy = policy;
        self
    }

    /// Copies response metadata into a mutable interceptor context.
    ///
    /// # Parameters
    /// - `meta`: Source response metadata.
    ///
    /// # Returns
    /// New context with cloned headers, URL, and method.
    pub fn from_meta(meta: &HttpResponseMeta) -> Self {
        Self::new(
            meta.status(),
            meta.headers().clone(),
            meta.url().clone(),
            meta.method().clone(),
        )
        .with_log_sanitize_policy(meta.log_sanitize_policy().clone())
    }

    /// Returns response status code.
    ///
    /// # Returns
    /// Immutable status accepted by `HttpClient::execute`.
    #[inline]
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns response headers.
    ///
    /// # Returns
    /// Immutable header map view.
    #[inline]
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Returns mutable response headers.
    ///
    /// # Returns
    /// Mutable header map applied back to [`HttpResponseMeta`] after all
    /// response interceptors succeed.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }

    /// Returns final response URL.
    ///
    /// # Returns
    /// Immutable response URL view.
    #[inline]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Replaces final response URL.
    ///
    /// # Parameters
    /// - `url`: New final response URL.
    ///
    /// # Returns
    /// `self` for method chaining.
    #[inline]
    pub fn set_url(&mut self, url: Url) -> &mut Self {
        self.url = url;
        self
    }

    /// Returns originating request method.
    ///
    /// # Returns
    /// Immutable request method.
    #[inline]
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Returns parsed `Retry-After` when status and headers provide one.
    ///
    /// # Returns
    /// `Some(Duration)` for retryable status codes with valid `Retry-After`;
    /// otherwise `None`.
    #[inline]
    pub fn retry_after_hint(&self) -> Option<Duration> {
        HttpResponseMeta::retry_after_hint_from_parts(self.status, &self.headers)
    }

    /// Applies mutable context fields back into response metadata.
    ///
    /// # Parameters
    /// - `meta`: Response metadata to update.
    ///
    /// # Returns
    /// Nothing. Status and method are intentionally not copied back.
    pub(super) fn apply_to_meta(self, meta: &mut HttpResponseMeta) {
        meta.set_headers(self.headers);
        meta.set_url(self.url);
        meta.set_log_sanitize_policy(self.log_sanitize_policy);
    }
}

impl fmt::Debug for HttpResponseInterceptorContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sanitizer = crate::LogSanitizer::for_debug(&self.log_sanitize_policy);
        let url = sanitizer.sanitize_url(&self.url);
        formatter
            .debug_struct("HttpResponseInterceptorContext")
            .field("status", &self.status)
            .field("headers", &sanitizer.sanitize_header_map(&self.headers))
            .field("url", &url)
            .field("method", &self.method)
            .finish()
    }
}
