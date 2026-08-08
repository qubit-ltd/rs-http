// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Response interceptor context with controlled metadata mutation.

use std::fmt;
use std::time::Duration;

use http::HeaderMap;
use http::Method;
use http::StatusCode;
use qubit_redact::RedactionPolicy;
use qubit_redact::http::HttpRedactor;
use url::Url;

use super::HttpResponseMeta;
use crate::redact::RedactedDebugger;

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
    /// Redaction policy snapshot used by standalone debug output.
    log_redactor: HttpRedactor,
}

impl HttpResponseInterceptorContext {
    /// Creates a response interceptor context from explicit metadata parts.
    ///
    /// # Parameters
    /// - `status`: Response status code. It is immutable after construction.
    /// - `headers`: Response headers that interceptors may mutate.
    /// - `url`: Final response URL that interceptors may replace.
    /// - `method`: Originating request method. It is immutable after
    ///   construction.
    ///
    /// # Returns
    /// New interceptor context.
    #[inline]
    pub fn new(
        status: StatusCode,
        headers: HeaderMap,
        url: Url,
        method: Method,
    ) -> Self {
        Self {
            status,
            headers,
            url,
            method,
            log_redactor: HttpRedactor::new(RedactionPolicy::default()),
        }
    }

    /// Copies response metadata into a mutable interceptor context.
    ///
    /// # Parameters
    /// - `meta`: Source response metadata.
    ///
    /// # Returns
    /// New context with cloned headers, URL, method, and policy snapshot.
    #[inline(always)]
    pub fn from_meta(meta: &HttpResponseMeta) -> Self {
        Self::new(
            meta.status(),
            meta.headers().clone(),
            meta.url().clone(),
            meta.method().clone(),
        )
        .with_log_redactor(meta.log_redactor().clone())
    }

    /// Attaches the shared log redactor used for standalone debug output.
    ///
    /// # Parameters
    /// - `log_redactor`: Shared log redactor to apply when formatting this
    ///   context.
    ///
    /// # Returns
    /// Updated context.
    #[inline(always)]
    pub fn with_log_redactor(mut self, log_redactor: HttpRedactor) -> Self {
        self.log_redactor = log_redactor;
        self
    }

    /// Attaches a policy snapshot used for standalone debug output.
    ///
    /// # Parameters
    /// - `policy`: Policy snapshot to apply when formatting this context.
    ///
    /// # Returns
    /// Updated context.
    #[inline(always)]
    pub fn with_log_redaction_policy(
        mut self,
        policy: RedactionPolicy,
    ) -> Self {
        self.log_redactor = HttpRedactor::new(policy);
        self
    }

    /// Returns response status code.
    ///
    /// # Returns
    /// Immutable status accepted by `HttpClient::execute`.
    #[inline(always)]
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns response headers.
    ///
    /// # Returns
    /// Immutable header map view.
    #[inline(always)]
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Returns mutable response headers.
    ///
    /// # Returns
    /// Mutable header map applied back to [`HttpResponseMeta`] after all
    /// response interceptors succeed.
    #[inline(always)]
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }

    /// Returns final response URL.
    ///
    /// # Returns
    /// Immutable response URL view.
    #[inline(always)]
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
    #[inline(always)]
    pub fn set_url(&mut self, url: Url) -> &mut Self {
        self.url = url;
        self
    }

    /// Returns originating request method.
    ///
    /// # Returns
    /// Immutable request method.
    #[inline(always)]
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Returns parsed `Retry-After` when status and headers provide one.
    ///
    /// # Returns
    /// `Some(Duration)` for retryable status codes with valid `Retry-After`;
    /// otherwise `None`.
    #[inline(always)]
    pub fn retry_after_hint(&self) -> Option<Duration> {
        HttpResponseMeta::retry_after_hint_from_parts(
            self.status,
            &self.headers,
        )
    }

    /// Applies mutable context fields back into response metadata.
    ///
    /// # Parameters
    /// - `meta`: Response metadata to update.
    ///
    /// # Returns
    /// Nothing. Status and method are intentionally not copied back.
    #[inline(always)]
    pub(super) fn apply_to_meta(self, meta: &mut HttpResponseMeta) {
        meta.set_headers(self.headers);
        meta.set_url(self.url);
        meta.set_log_redactor(self.log_redactor);
    }
}

impl fmt::Debug for HttpResponseInterceptorContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let debugger = RedactedDebugger::new(&self.log_redactor);
        let session = debugger.session();
        let url = debugger.url_with_session(&self.url, &session);
        formatter
            .debug_struct("HttpResponseInterceptorContext")
            .field("status", &self.status)
            .field(
                "headers",
                &debugger.headers_with_session(&self.headers, &session),
            )
            .field("url", &url)
            .field("method", &self.method)
            .finish()
    }
}
