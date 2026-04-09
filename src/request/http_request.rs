/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Immutable HTTP request object.

use std::time::Duration;

use http::{HeaderMap, Method};

use super::http_request_body::HttpRequestBody;

/// Immutable snapshot of a single HTTP call produced by [`crate::HttpRequestBuilder`].
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// HTTP method (GET, POST, …).
    pub method: Method,
    /// Absolute URL string, or path joined with client `base_url` when not parseable as URL.
    pub path: String,
    /// Query string parameters as `(name, value)` pairs.
    pub query: Vec<(String, String)>,
    /// Headers added on top of client defaults and injector output.
    pub headers: HeaderMap,
    /// Serialized body variant.
    pub body: HttpRequestBody,
    /// Overrides client-wide request timeout when set; otherwise client default applies.
    pub request_timeout: Option<Duration>,
}
