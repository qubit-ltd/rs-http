/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Shared HTTP response metadata (status, headers, URL, request method).

use http::{HeaderMap, Method, StatusCode};
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
        }
    }
}
