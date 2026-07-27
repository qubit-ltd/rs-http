// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared HTTP response metadata (status, headers, URL, request method).

use std::fmt;
use std::time::{Duration, SystemTime};

use http::header::RETRY_AFTER;
use http::{HeaderMap, Method, StatusCode};
use httpdate::parse_http_date;
use url::Url;

use crate::redact::RedactedDebugger;
use crate::LogRedactionPolicy;

/// HTTP response metadata available before body buffering/stream consumption.
#[derive(Clone)]
pub struct HttpResponseMeta {
    /// Response status code.
    status: StatusCode,
    /// Response headers.
    headers: HeaderMap,
    /// Final resolved URL.
    url: Url,
    /// Originating request method.
    method: Method,
    /// Redaction policy snapshot used by standalone debug output.
    log_redaction_policy: LogRedactionPolicy,
}

impl HttpResponseMeta {
    /// Creates response metadata from status/headers/url/method parts.
    #[inline]
    pub fn new(status: StatusCode, headers: HeaderMap, url: Url, method: Method) -> Self {
        Self {
            status,
            headers,
            url,
            method,
            log_redaction_policy: LogRedactionPolicy::default(),
        }
    }

    /// Attaches the log redaction policy used for standalone debug output.
    ///
    /// # Parameters
    /// - `policy`: Policy snapshot to apply when formatting this metadata.
    ///
    /// # Returns
    /// Updated metadata.
    #[inline(always)]
    pub fn with_log_redaction_policy(mut self, policy: LogRedactionPolicy) -> Self {
        self.log_redaction_policy = policy;
        self
    }

    /// Returns response status code.
    ///
    /// # Returns
    /// Immutable response status.
    #[inline(always)]
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns response headers.
    ///
    /// # Returns
    /// Immutable response header map.
    #[inline(always)]
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Returns final response URL.
    ///
    /// # Returns
    /// Immutable final response URL.
    #[inline(always)]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Returns originating request method.
    ///
    /// # Returns
    /// Immutable request method.
    #[inline(always)]
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Returns parsed `Retry-After` when this response status should honor it.
    ///
    /// Applicable statuses are `429` and `5xx`, and header value can be
    /// `delta-seconds` or HTTP-date.
    #[inline(always)]
    pub fn retry_after_hint(&self) -> Option<Duration> {
        Self::retry_after_hint_from_parts(self.status, &self.headers)
    }

    /// Returns parsed `Retry-After` for explicit status/header parts.
    ///
    /// # Parameters
    /// - `status`: Response status code to inspect.
    /// - `headers`: Response headers that may contain `Retry-After`.
    ///
    /// # Returns
    /// `Some(Duration)` when status and header value are applicable; otherwise
    /// `None`.
    pub(super) fn retry_after_hint_from_parts(
        status: StatusCode,
        headers: &HeaderMap,
    ) -> Option<Duration> {
        if !is_retry_after_applicable_status(status) {
            return None;
        }
        headers
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_retry_after_value)
    }

    /// Replaces response headers after response interceptors complete.
    ///
    /// # Parameters
    /// - `headers`: New response headers.
    ///
    /// # Returns
    /// Nothing.
    #[inline(always)]
    pub(super) fn set_headers(&mut self, headers: HeaderMap) {
        self.headers = headers;
    }

    /// Replaces final response URL after response interceptors complete.
    ///
    /// # Parameters
    /// - `url`: New final response URL.
    ///
    /// # Returns
    /// Nothing.
    #[inline(always)]
    pub(super) fn set_url(&mut self, url: Url) {
        self.url = url;
    }

    /// Returns the log redaction policy snapshot for response diagnostics.
    ///
    /// # Returns
    /// Borrowed policy snapshot.
    #[inline(always)]
    pub(super) fn log_redaction_policy(&self) -> &LogRedactionPolicy {
        &self.log_redaction_policy
    }

    /// Replaces the log redaction policy snapshot.
    ///
    /// # Parameters
    /// - `policy`: New policy snapshot.
    ///
    /// # Returns
    /// Nothing.
    #[inline(always)]
    pub(super) fn set_log_redaction_policy(&mut self, policy: LogRedactionPolicy) {
        self.log_redaction_policy = policy;
    }
}

impl fmt::Debug for HttpResponseMeta {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let debugger = RedactedDebugger::new(&self.log_redaction_policy);
        let url = debugger.url(&self.url);
        formatter
            .debug_struct("HttpResponseMeta")
            .field("status", &self.status)
            .field("headers", &debugger.headers(&self.headers))
            .field("url", &url)
            .field("method", &self.method)
            .finish()
    }
}

fn is_retry_after_applicable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn parse_retry_after_value(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(seconds) = trimmed.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let retry_at = parse_http_date(trimmed).ok()?;
    let now = SystemTime::now();
    Some(
        retry_at
            .duration_since(now)
            .unwrap_or_else(|_| Duration::from_secs(0)),
    )
}
