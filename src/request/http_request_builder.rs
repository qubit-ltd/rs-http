/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Builder for [`super::http_request::HttpRequest`].

use std::time::Duration;

use bytes::Bytes;
use http::header::CONTENT_TYPE;
use http::{HeaderMap, HeaderName, HeaderValue, Method};
use serde::Serialize;

use crate::{HttpError, HttpResult};

use super::http_request::HttpRequest;
use super::http_request_body::HttpRequestBody;

/// Builder for [`HttpRequest`](super::http_request::HttpRequest).
#[derive(Debug, Clone)]
pub struct HttpRequestBuilder {
    /// HTTP method (e.g. GET, POST).
    method: Method,
    /// Request path without the query string.
    path: String,
    /// Query parameters as `(key, value)` pairs, appended to the URL when built.
    query: Vec<(String, String)>,
    /// Request headers.
    headers: HeaderMap,
    /// Request body; empty if not set.
    body: HttpRequestBody,
    /// Per-request timeout; if unset, the client default applies.
    request_timeout: Option<Duration>,
}

impl HttpRequestBuilder {
    /// Starts a builder with method and path; body empty, no query, no extra headers.
    ///
    /// # Parameters
    /// - `method`: HTTP verb.
    /// - `path`: URL or relative path string.
    ///
    /// # Returns
    /// New [`HttpRequestBuilder`].
    pub fn new(method: Method, path: impl AsRef<str>) -> Self {
        Self {
            method,
            path: path.as_ref().to_string(),
            query: Vec::new(),
            headers: HeaderMap::new(),
            body: HttpRequestBody::Empty,
            request_timeout: None,
        }
    }

    /// Appends a single `key=value` query pair (order preserved).
    ///
    /// # Parameters
    /// - `key`: Query parameter name.
    /// - `value`: Query parameter value.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn query_param(mut self, key: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        self.query
            .push((key.as_ref().to_string(), value.as_ref().to_string()));
        self
    }

    /// Appends many query pairs via [`HttpRequestBuilder::query_param`].
    ///
    /// # Parameters
    /// - `params`: Iterator of `(key, value)` pairs.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn query_params<I, K, V>(mut self, params: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        for (key, value) in params {
            self = self.query_param(key, value);
        }
        self
    }

    /// Validates and inserts one header.
    ///
    /// # Parameters
    /// - `name`: Header name (must be valid [`http::header::HeaderName`] bytes).
    /// - `value`: Header value (must be valid [`http::header::HeaderValue`]).
    ///
    /// # Returns
    /// `Ok(self)` or [`HttpError`] if name/value are invalid.
    pub fn header(mut self, name: impl AsRef<str>, value: impl AsRef<str>) -> HttpResult<Self> {
        let header_name = HeaderName::from_bytes(name.as_ref().as_bytes()).map_err(|error| {
            HttpError::other(format!(
                "Invalid header name '{}': {}",
                name.as_ref(),
                error
            ))
        })?;
        let header_value = HeaderValue::from_str(value.as_ref()).map_err(|error| {
            HttpError::other(format!(
                "Invalid header value for '{}': {}",
                name.as_ref(),
                error
            ))
        })?;
        self.headers.insert(header_name, header_value);
        Ok(self)
    }

    /// Merges all entries from `headers` into this builder (existing names may get extra values).
    ///
    /// # Parameters
    /// - `headers`: Map to append.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.headers.extend(headers);
        self
    }

    /// Sets the body to raw bytes without changing `Content-Type` unless already set elsewhere.
    ///
    /// # Parameters
    /// - `body`: Payload.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn bytes_body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = HttpRequestBody::Bytes(body.into());
        self
    }

    /// Sets a UTF-8 text body and adds `text/plain; charset=utf-8` if `Content-Type` is absent.
    ///
    /// # Parameters
    /// - `body`: Text payload.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn text_body(mut self, body: impl Into<String>) -> Self {
        if !self.headers.contains_key(CONTENT_TYPE) {
            self.headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            );
        }
        self.body = HttpRequestBody::Text(body.into());
        self
    }

    /// Serializes `value` to JSON, sets body to those bytes, and adds `application/json` if needed.
    ///
    /// # Parameters
    /// - `value`: Serializable value.
    ///
    /// # Returns
    /// `Ok(self)` or [`HttpError`] if JSON encoding fails.
    pub fn json_body<T>(mut self, value: &T) -> HttpResult<Self>
    where
        T: Serialize,
    {
        let bytes = serde_json::to_vec(value)
            .map_err(|error| HttpError::decode(format!("Failed to encode JSON body: {}", error)))?;
        if !self.headers.contains_key(CONTENT_TYPE) {
            self.headers
                .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }
        self.body = HttpRequestBody::Json(Bytes::from(bytes));
        Ok(self)
    }

    /// Overrides the client-wide request timeout for this request only.
    ///
    /// # Parameters
    /// - `timeout`: Maximum time for the whole request (reqwest `timeout`).
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = Some(timeout);
        self
    }

    /// Consumes the builder into a frozen [`HttpRequest`].
    ///
    /// # Returns
    /// Built [`HttpRequest`].
    pub fn build(self) -> HttpRequest {
        HttpRequest {
            method: self.method,
            path: self.path,
            query: self.query,
            headers: self.headers,
            body: self.body,
            request_timeout: self.request_timeout,
        }
    }
}
