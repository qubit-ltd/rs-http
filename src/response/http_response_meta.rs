/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Shared HTTP response metadata (status, headers, URL).

use http::{HeaderMap, StatusCode};
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
}

impl HttpResponseMeta {
    /// Creates response metadata from status/headers/url parts.
    pub fn new(status: StatusCode, headers: HeaderMap, url: Url) -> Self {
        Self {
            status,
            headers,
            url,
        }
    }
}
