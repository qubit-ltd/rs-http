/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Shared HTTP response metadata (status, headers, URL, request method).

use std::time::{Duration, SystemTime};

use http::header::RETRY_AFTER;
use http::{HeaderMap, Method, StatusCode};
use httpdate::parse_http_date;
use url::Url;

/// HTTP response metadata available before body buffering/stream consumption.
#[derive(Debug, Clone)]
pub struct HttpResponseMeta {
    /// Response status code.
    pub status: StatusCode,
    /// Response headers.
    pub headers: HeaderMap,
    /// Final resolved URL.
    pub url: Url,
    /// Originating request method.
    pub method: Method,
}

impl HttpResponseMeta {
    /// Creates response metadata from status/headers/url/method parts.
    pub fn new(status: StatusCode, headers: HeaderMap, url: Url, method: Method) -> Self {
        Self {
            status,
            headers,
            url,
            method,
        }
    }

    /// Returns parsed `Retry-After` when this response status should honor it.
    ///
    /// Applicable statuses are `429` and `5xx`, and header value can be
    /// `delta-seconds` or HTTP-date.
    pub fn retry_after_hint(&self) -> Option<Duration> {
        if !is_retry_after_applicable_status(self.status) {
            return None;
        }
        self.headers
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_retry_after_value)
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
