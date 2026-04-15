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
use http::{HeaderMap, HeaderValue, Method};
use serde::Serialize;
use tokio_util::sync::CancellationToken;
use url::form_urlencoded;
use url::Url;

use crate::{
    AsyncHttpHeaderInjector, HttpClient, HttpError, HttpHeaderInjector, HttpResult,
    HttpRetryMethodPolicy,
};

use super::http_request::HttpRequest;
use super::http_request_body::HttpRequestBody;
use super::http_request_retry_override::HttpRequestRetryOverride;
use super::parse_header;

/// Builder for [`HttpRequest`](super::http_request::HttpRequest).
#[derive(Debug, Clone)]
pub struct HttpRequestBuilder {
    /// HTTP method (e.g. GET, POST).
    pub(super) method: Method,
    /// Request path without the query string.
    pub(super) path: String,
    /// Query parameters as `(key, value)` pairs, appended to the URL when built.
    pub(super) query: Vec<(String, String)>,
    /// Request headers.
    pub(super) headers: HeaderMap,
    /// Request body; empty if not set.
    pub(super) body: HttpRequestBody,
    /// Per-request timeout; if unset, the client default applies.
    pub(super) request_timeout: Option<Duration>,
    /// Per-request write timeout used by the send phase.
    pub(super) write_timeout: Duration,
    /// Per-request read timeout used by buffered/stream response reading.
    pub(super) read_timeout: Duration,
    /// Base URL copied from client options and used by [`HttpRequest::resolve_url`].
    pub(super) base_url: Option<Url>,
    /// Whether IPv6 literal hosts are rejected during URL resolution.
    pub(super) ipv4_only: bool,
    /// Optional cancellation token for this request.
    pub(super) cancellation_token: Option<CancellationToken>,
    /// Per-request retry override for one-off retry behavior customization.
    pub(super) retry_override: HttpRequestRetryOverride,
    /// Default headers snapshot from the originating client.
    pub(super) default_headers: HeaderMap,
    /// Sync header injectors snapshot from the originating client.
    pub(super) injectors: Vec<HttpHeaderInjector>,
    /// Async header injectors snapshot from the originating client.
    pub(super) async_injectors: Vec<AsyncHttpHeaderInjector>,
}

impl HttpRequestBuilder {
    /// Starts a builder with method/path and copies supported defaults from client options.
    ///
    /// # Parameters
    /// - `method`: HTTP verb.
    /// - `path`: URL or relative path string.
    /// - `client`: Source client whose relevant defaults are copied into this builder.
    ///
    /// # Returns
    /// New [`HttpRequestBuilder`].
    pub(crate) fn new(method: Method, path: &str, client: &HttpClient) -> Self {
        let options = client.options();
        Self {
            method,
            path: path.to_string(),
            query: Vec::new(),
            headers: HeaderMap::new(),
            body: HttpRequestBody::Empty,
            request_timeout: options.timeouts.request_timeout,
            write_timeout: options.timeouts.write_timeout,
            read_timeout: options.timeouts.read_timeout,
            base_url: options.base_url.clone(),
            ipv4_only: options.ipv4_only,
            cancellation_token: None,
            retry_override: HttpRequestRetryOverride::default(),
            default_headers: client.headers_snapshot(),
            injectors: client.injectors_snapshot(),
            async_injectors: client.async_injectors_snapshot(),
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
    pub fn query_param(mut self, key: &str, value: &str) -> Self {
        self.query.push((key.to_string(), value.to_string()));
        self
    }

    /// Appends many query pairs via [`HttpRequestBuilder::query_param`].
    ///
    /// # Parameters
    /// - `params`: Iterator of `(key, value)` pairs.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn query_params<'a, I>(mut self, params: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
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
    pub fn header(mut self, name: &str, value: &str) -> HttpResult<Self> {
        let (header_name, header_value) = parse_header(name, value)?;
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

    /// Sets the body to an ordered chunk stream for incremental upload.
    ///
    /// # Parameters
    /// - `chunks`: Iterator of chunks in send order.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn stream_body<I, B>(mut self, chunks: I) -> Self
    where
        I: IntoIterator<Item = B>,
        B: Into<Bytes>,
    {
        self.body = HttpRequestBody::Stream(chunks.into_iter().map(Into::into).collect());
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

    /// Serializes key-value pairs as `application/x-www-form-urlencoded`.
    ///
    /// # Parameters
    /// - `fields`: Iterable of `(key, value)` string pairs.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn form_body<'a, I>(mut self, fields: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        for (key, value) in fields {
            serializer.append_pair(key, value);
        }
        let body = serializer.finish();
        if !self.headers.contains_key(CONTENT_TYPE) {
            self.headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/x-www-form-urlencoded"),
            );
        }
        self.body = HttpRequestBody::Form(Bytes::from(body));
        self
    }

    /// Sets multipart body bytes and optional auto content-type by boundary.
    ///
    /// # Parameters
    /// - `body`: Multipart payload bytes.
    /// - `boundary`: Multipart boundary used in payload framing.
    ///
    /// # Returns
    /// `Ok(self)` for chaining.
    ///
    /// # Errors
    /// Returns [`HttpError`] when `boundary` is empty or content-type cannot be built.
    pub fn multipart_body(mut self, body: impl Into<Bytes>, boundary: &str) -> HttpResult<Self> {
        if boundary.trim().is_empty() {
            return Err(HttpError::other(
                "Multipart boundary cannot be empty for multipart_body",
            ));
        }
        if !self.headers.contains_key(CONTENT_TYPE) {
            let value = HeaderValue::from_str(&format!("multipart/form-data; boundary={boundary}"))
                .map_err(|error| {
                    HttpError::other(format!(
                        "Invalid multipart Content-Type header value: {error}"
                    ))
                })?;
            self.headers.insert(CONTENT_TYPE, value);
        }
        self.body = HttpRequestBody::Multipart(body.into());
        Ok(self)
    }

    /// Serializes records as NDJSON (`one JSON object per line`).
    ///
    /// # Parameters
    /// - `records`: Serializable records to encode as NDJSON lines.
    ///
    /// # Returns
    /// `Ok(self)` for chaining.
    ///
    /// # Errors
    /// Returns [`HttpError`] when any record fails JSON serialization.
    pub fn ndjson_body<T>(mut self, records: &[T]) -> HttpResult<Self>
    where
        T: Serialize,
    {
        let mut payload = String::new();
        for record in records {
            let line = serde_json::to_string(record).map_err(|error| {
                HttpError::decode(format!("Failed to encode NDJSON record: {error}"))
            })?;
            payload.push_str(&line);
            payload.push('\n');
        }
        if !self.headers.contains_key(CONTENT_TYPE) {
            self.headers.insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/x-ndjson"),
            );
        }
        self.body = HttpRequestBody::Ndjson(Bytes::from(payload));
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

    /// Overrides the write-phase timeout for this request only.
    ///
    /// # Parameters
    /// - `timeout`: Maximum time allowed for sending the request bytes.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn write_timeout(mut self, timeout: Duration) -> Self {
        self.write_timeout = timeout;
        self
    }

    /// Overrides the read-phase timeout for this request only.
    ///
    /// # Parameters
    /// - `timeout`: Maximum time allowed for one read wait on response body.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = timeout;
        self
    }

    /// Overrides the client default base URL for this request.
    ///
    /// # Parameters
    /// - `base_url`: Base URL used when resolving relative request paths.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn base_url(mut self, base_url: Url) -> Self {
        self.base_url = Some(base_url);
        self
    }

    /// Clears the base URL for this request.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn clear_base_url(mut self) -> Self {
        self.base_url = None;
        self
    }

    /// Overrides whether this request enforces IPv4-only literal-host validation.
    ///
    /// # Parameters
    /// - `enabled`: `true` to reject IPv6 literal hosts, `false` to allow them.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn ipv4_only(mut self, enabled: bool) -> Self {
        self.ipv4_only = enabled;
        self
    }

    /// Binds a [`CancellationToken`] to this request.
    ///
    /// # Parameters
    /// - `token`: Cancellation token checked before send and during request/stream I/O.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn cancellation_token(mut self, token: CancellationToken) -> Self {
        self.cancellation_token = Some(token);
        self
    }

    /// Forces retry enabled for this request even if client-level retry is disabled.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn force_retry(mut self) -> Self {
        self.retry_override = self.retry_override.force_enable();
        self
    }

    /// Disables retry for this request even if client-level retry is enabled.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn disable_retry(mut self) -> Self {
        self.retry_override = self.retry_override.force_disable();
        self
    }

    /// Overrides retryable-method policy for this request.
    ///
    /// # Parameters
    /// - `policy`: Method policy to apply on this request only.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn retry_method_policy(mut self, policy: HttpRetryMethodPolicy) -> Self {
        self.retry_override = self.retry_override.with_method_policy(policy);
        self
    }

    /// Enables or disables honoring `Retry-After` for this request.
    ///
    /// # Parameters
    /// - `enabled`: `true` to honor `Retry-After` on retryable status
    ///   responses (`429` and `5xx`).
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn honor_retry_after(mut self, enabled: bool) -> Self {
        self.retry_override = self.retry_override.with_honor_retry_after(enabled);
        self
    }

    /// Consumes the builder into a frozen [`HttpRequest`].
    ///
    /// # Returns
    /// Built [`HttpRequest`].
    pub fn build(self) -> HttpRequest {
        HttpRequest::new(self)
    }
}
