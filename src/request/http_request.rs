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

use bytes::Bytes;
use futures_util::stream as futures_stream;
use http::{HeaderMap, HeaderName, HeaderValue, Method};
use qubit_function::MutatingFunction;
use reqwest::Response;
use tokio_util::sync::CancellationToken;
use url::Host;
use url::Url;

use crate::client::error_mapper::{map_reqwest_error, ReqwestErrorPhase};
use crate::{AsyncHeaderInjector, HeaderInjector, HttpError, HttpErrorKind, HttpResult};

use super::http_request_body::HttpRequestBody;
use super::http_request_builder::HttpRequestBuilder;
use super::http_request_retry_override::HttpRequestRetryOverride;
use super::parse_header;

/// Immutable snapshot of a single HTTP call produced by
/// [`crate::HttpRequestBuilder`].
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// HTTP method (GET, POST, …).
    method: Method,
    /// Absolute URL string, or path joined with client `base_url` when not
    /// parseable as URL.
    path: String,
    /// Query string parameters as `(name, value)` pairs.
    query: Vec<(String, String)>,
    /// Headers added on top of client defaults and injector output.
    headers: HeaderMap,
    /// Serialized body variant.
    body: HttpRequestBody,
    /// Overrides client-wide request timeout when set; otherwise client default
    /// applies.
    request_timeout: Option<Duration>,
    /// Per-request write timeout used during request sending.
    write_timeout: Duration,
    /// Base URL copied from client options, used to resolve relative `path`.
    base_url: Option<Url>,
    /// Whether resolved URLs must avoid IPv6 literal hosts.
    ipv4_only: bool,
    /// Optional cancellation token checked before send and during I/O phases.
    cancellation_token: Option<CancellationToken>,
    /// Per-request retry override (enable/disable/method-policy/Retry-After
    /// behavior).
    retry_override: HttpRequestRetryOverride,
    /// Client default headers snapshot captured when this request builder was
    /// created.
    default_headers: HeaderMap,
    /// Client sync header injectors snapshot captured when this request builder
    /// was created.
    injectors: Vec<HeaderInjector>,
    /// Client async header injectors snapshot captured when this request
    /// builder was created.
    async_injectors: Vec<AsyncHeaderInjector>,
}

impl HttpRequest {
    /// Consumes a finished [`HttpRequestBuilder`] and freezes its fields into
    /// an [`HttpRequest`].
    ///
    /// # Parameters
    /// - `builder`: Populated builder produced by the HTTP client pipeline.
    ///
    /// # Returns
    /// Snapshot ready for URL resolution, header assembly, and sending.
    pub(super) fn new(builder: HttpRequestBuilder) -> Self {
        Self {
            method: builder.method,
            path: builder.path,
            query: builder.query,
            headers: builder.headers,
            body: builder.body,
            request_timeout: builder.request_timeout,
            write_timeout: builder.write_timeout,
            base_url: builder.base_url,
            ipv4_only: builder.ipv4_only,
            cancellation_token: builder.cancellation_token,
            retry_override: builder.retry_override,
            default_headers: builder.default_headers,
            injectors: builder.injectors,
            async_injectors: builder.async_injectors,
        }
    }

    /// Returns the HTTP verb for this snapshot.
    ///
    /// # Returns
    /// Borrowed [`Method`] (for example GET or POST).
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Replaces the HTTP verb.
    ///
    /// # Parameters
    /// - `method`: New [`Method`].
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn set_method(&mut self, method: Method) -> &mut Self {
        self.method = method;
        self
    }

    /// Returns the path segment or absolute URL string stored on this request.
    ///
    /// # Returns
    /// The raw path/URL before query string assembly; may be relative if a base
    /// URL is set.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Replaces the path or absolute URL string.
    ///
    /// # Parameters
    /// - `path`: New path or URL string (query string is managed separately via
    ///   [`Self::add_query_param`]).
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn set_path(&mut self, path: impl Into<String>) -> &mut Self {
        self.path = path.into();
        self
    }

    /// Returns ordered `(name, value)` query pairs that will be appended to the
    /// resolved URL.
    ///
    /// # Returns
    /// Slice view of accumulated query parameters.
    pub fn query(&self) -> &[(String, String)] {
        &self.query
    }

    /// Appends a single query pair preserving insertion order.
    ///
    /// # Parameters
    /// - `key`: Parameter name.
    /// - `value`: Parameter value.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn add_query_param(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> &mut Self {
        self.query.push((key.into(), value.into()));
        self
    }

    /// Removes every query pair from this snapshot.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn clear_query_params(&mut self) -> &mut Self {
        self.query.clear();
        self
    }

    /// Returns request-local headers layered on top of client defaults and
    /// injector output at send time.
    ///
    /// # Returns
    /// Borrowed [`HeaderMap`] owned by this request only (not merged defaults).
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Parses and inserts one header from string name/value pairs.
    ///
    /// # Parameters
    /// - `name`: Header field name.
    /// - `value`: Header field value.
    ///
    /// # Returns
    /// `Ok(self)` on success.
    ///
    /// # Errors
    /// Returns [`HttpError`] when name or value cannot be converted into valid
    /// HTTP tokens.
    pub fn set_header(&mut self, name: &str, value: &str) -> Result<&mut Self, HttpError> {
        let (header_name, header_value) = parse_header(name, value)?;
        self.headers.insert(header_name, header_value);
        Ok(self)
    }

    /// Inserts one header using pre-validated [`HeaderName`] / [`HeaderValue`]
    /// types.
    ///
    /// # Parameters
    /// - `name`: Typed header name.
    /// - `value`: Typed header value.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn set_typed_header(&mut self, name: HeaderName, value: HeaderValue) -> &mut Self {
        self.headers.insert(name, value);
        self
    }

    /// Removes all values for a header field by typed name.
    ///
    /// # Parameters
    /// - `name`: Header name to strip from the request-local map.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn remove_header(&mut self, name: &HeaderName) -> &mut Self {
        self.headers.remove(name);
        self
    }

    /// Clears all request-local headers (defaults and injectors are unaffected
    /// until send).
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn clear_headers(&mut self) -> &mut Self {
        self.headers.clear();
        self
    }

    /// Returns the serialized body variant for this snapshot.
    ///
    /// # Returns
    /// Borrowed [`HttpRequestBody`].
    pub fn body(&self) -> &HttpRequestBody {
        &self.body
    }

    /// Replaces the entire body payload.
    ///
    /// # Parameters
    /// - `body`: New [`HttpRequestBody`] variant.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn set_body(&mut self, body: HttpRequestBody) -> &mut Self {
        self.body = body;
        self
    }

    /// Returns the per-request total timeout, if any.
    ///
    /// # Returns
    /// `Some(duration)` when a request-specific timeout overrides the client
    /// default; otherwise `None`.
    pub fn request_timeout(&self) -> Option<Duration> {
        self.request_timeout
    }

    /// Sets a per-request total timeout that overrides the client default for
    /// this send.
    ///
    /// # Parameters
    /// - `timeout`: Upper bound for the entire request lifecycle handled by
    ///   reqwest.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn set_request_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.request_timeout = Some(timeout);
        self
    }

    /// Drops the per-request timeout so the client-wide default applies again.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn clear_request_timeout(&mut self) -> &mut Self {
        self.request_timeout = None;
        self
    }

    /// Returns the write-phase timeout used while sending the request.
    pub fn write_timeout(&self) -> Duration {
        self.write_timeout
    }

    /// Sets the write-phase timeout used while sending the request.
    pub fn set_write_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.write_timeout = timeout;
        self
    }

    /// Returns the optional base URL used to resolve relative [`Self::path`]
    /// values.
    ///
    /// # Returns
    /// `Some` when a base is configured; `None` when only absolute URLs in
    /// `path` are valid.
    pub fn base_url(&self) -> Option<&Url> {
        self.base_url.as_ref()
    }

    /// Sets the base URL used by [`Self::resolve_url`] when `path` is not
    /// absolute.
    ///
    /// # Parameters
    /// - `base_url`: Root URL to join against relative paths.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn set_base_url(&mut self, base_url: Url) -> &mut Self {
        self.base_url = Some(base_url);
        self
    }

    /// Removes the configured base URL so relative paths can no longer be
    /// resolved without resetting it.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn clear_base_url(&mut self) -> &mut Self {
        self.base_url = None;
        self
    }

    /// Returns whether IPv6 literal hosts are rejected after URL resolution.
    ///
    /// # Returns
    /// `true` when a resolved URL whose host is an IPv6 literal must be
    /// rejected with [`HttpError::invalid_url`].
    pub fn ipv4_only(&self) -> bool {
        self.ipv4_only
    }

    /// Enables or disables IPv6 literal host rejection for resolved URLs.
    ///
    /// # Parameters
    /// - `enabled`: When `true`, resolved URLs whose host is an IPv6 literal
    ///   are errors.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn set_ipv4_only(&mut self, enabled: bool) -> &mut Self {
        self.ipv4_only = enabled;
        self
    }

    /// Returns the cooperative cancellation handle, if configured.
    ///
    /// # Returns
    /// `Some` token checked before send and during I/O; `None` when
    /// cancellation is not wired.
    pub fn cancellation_token(&self) -> Option<&CancellationToken> {
        self.cancellation_token.as_ref()
    }

    /// Attaches a [`CancellationToken`] that can abort this request
    /// cooperatively.
    ///
    /// # Parameters
    /// - `token`: Shared cancellation source.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn set_cancellation_token(&mut self, token: CancellationToken) -> &mut Self {
        self.cancellation_token = Some(token);
        self
    }

    /// Removes any cancellation token from this snapshot.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn clear_cancellation_token(&mut self) -> &mut Self {
        self.cancellation_token = None;
        self
    }

    /// Returns the per-request retry override applied by the client pipeline.
    ///
    /// # Returns
    /// Borrowed [`HttpRequestRetryOverride`].
    pub fn retry_override(&self) -> &HttpRequestRetryOverride {
        &self.retry_override
    }

    /// Replaces the retry override for this single request.
    ///
    /// # Parameters
    /// - `retry_override`: New override policy and knobs.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn set_retry_override(&mut self, retry_override: HttpRequestRetryOverride) -> &mut Self {
        self.retry_override = retry_override;
        self
    }

    /// Moves the current body out, leaving [`HttpRequestBody::Empty`] in its
    /// place.
    ///
    /// Used internally before handing the payload to reqwest so the snapshot is
    /// not cloned twice.
    ///
    /// # Returns
    /// Previous [`HttpRequestBody`] value.
    pub(crate) fn take_body(&mut self) -> HttpRequestBody {
        std::mem::replace(&mut self.body, HttpRequestBody::Empty)
    }

    /// Assembles a reqwest [`RequestBuilder`](reqwest::RequestBuilder), applies
    /// this snapshot's body, then sends with a bounded write phase.
    ///
    /// Centralizes query/timeout/body wiring plus cooperative cancellation and
    /// write-timeout handling; higher-level retry, logging, and interceptors
    /// stay in [`crate::HttpClient`].
    ///
    /// # Parameters
    /// - `backend`: Shared reqwest client.
    /// - `method`: Effective HTTP verb for this attempt (may differ from the
    ///   snapshot when retried).
    /// - `url`: Fully resolved request URL.
    /// - `headers`: Final merged header map from [`Self::build_headers`].
    ///
    /// # Returns
    /// The successful [`Response`] or a mapped [`HttpError`].
    ///
    /// # Errors
    /// - Cooperative cancellation while waiting on the send future.
    /// - Transport failures mapped from reqwest.
    /// - Write timeout when the send future does not complete within
    ///   `write_timeout`.
    pub(crate) async fn send_impl(
        &mut self,
        backend: &reqwest::Client,
        method: &Method,
        url: &Url,
        headers: HeaderMap,
    ) -> HttpResult<Response> {
        let mut builder = backend.request(method.clone(), url.clone());
        builder = builder.headers(headers);
        if !self.query.is_empty() {
            builder = builder.query(self.query.as_slice());
        }
        if let Some(timeout) = self.request_timeout {
            builder = builder.timeout(timeout);
        }
        builder = Self::apply_request_body(builder, self.take_body());

        let send_future = tokio::time::timeout(self.write_timeout, builder.send());
        let next = if let Some(token) = self.cancellation_token.as_ref() {
            tokio::select! {
                _ = token.cancelled() => {
                    return Err(HttpError::cancelled("Request cancelled while sending")
                        .with_method(method.clone())
                        .with_url(url.clone()));
                }
                send_result = send_future => send_result,
            }
        } else {
            send_future.await
        };

        match next {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(error)) => Err(map_reqwest_error(
                error,
                HttpErrorKind::Transport,
                Some(ReqwestErrorPhase::Send),
                Some(method.clone()),
                Some(url.clone()),
            )),
            Err(_) => Err(HttpError::write_timeout(format!(
                "Write timeout after {:?} while sending request",
                self.write_timeout
            ))
            .with_method(method.clone())
            .with_url(url.clone())),
        }
    }

    /// Parses [`Self::path`] as an absolute URL, or joins it with
    /// [`Self::base_url`] when the path is relative.
    ///
    /// # Returns
    /// Fully validated [`Url`] ready for transport.
    ///
    /// # Errors
    /// Returns [`HttpError::invalid_url`] when parsing fails, the base URL is
    /// missing for a relative path, joining fails, or [`Self::ipv4_only`]
    /// rejects an IPv6 literal host.
    pub(crate) fn resolve_url(&self) -> Result<Url, HttpError> {
        if let Ok(url) = Url::parse(&self.path) {
            self.validate_resolved_url_host(&url)?;
            return Ok(url);
        }

        let base = self.base_url.as_ref().ok_or_else(|| {
            HttpError::invalid_url(format!(
                "Cannot resolve relative path '{}' without base_url",
                self.path
            ))
        })?;

        let url = base.join(&self.path).map_err(|error| {
            HttpError::invalid_url(format!(
                "Failed to resolve path '{}' against base URL '{}': {}",
                self.path, base, error
            ))
        })?;
        self.validate_resolved_url_host(&url)?;
        Ok(url)
    }

    /// Enforces [`Self::ipv4_only`] by rejecting IPv6 literal hosts in `url`.
    ///
    /// # Parameters
    /// - `url`: Candidate URL after parsing or joining.
    ///
    /// # Returns
    /// `Ok(())` when the host is acceptable.
    ///
    /// # Errors
    /// [`HttpError::invalid_url`] when `ipv4_only` is `true` and the host is an
    /// IPv6 literal.
    fn validate_resolved_url_host(&self, url: &Url) -> Result<(), HttpError> {
        if self.ipv4_only && matches!(url.host(), Some(Host::Ipv6(_))) {
            return Err(HttpError::invalid_url(format!(
                "IPv6 literal host is not allowed when ipv4_only=true: {}",
                url
            )));
        }
        Ok(())
    }

    /// Builds the outbound [`HeaderMap`] by replaying client defaults and
    /// injectors, then layering request-local values.
    ///
    /// Merge order (later wins on duplicates):
    /// 1. Client default headers snapshot captured when the builder was
    ///    created.
    /// 2. Synchronous injector output in registration order.
    /// 3. Asynchronous injector output in registration order.
    /// 4. Request-local headers from this snapshot.
    ///
    /// # Returns
    /// Cloned map ready to attach to the reqwest builder.
    ///
    /// # Errors
    /// Propagates failures returned by any injector's `apply` implementation.
    pub(crate) async fn build_headers(&self) -> HttpResult<HeaderMap> {
        let mut headers = self.default_headers.clone();

        for injector in &self.injectors {
            injector.apply(&mut headers)?;
        }
        for injector in &self.async_injectors {
            injector.apply(&mut headers).await?;
        }

        headers.extend(self.headers.clone());
        Ok(headers)
    }

    /// Returns a pre-cancelled [`HttpError`] when a token is present and
    /// already cancelled.
    ///
    /// # Parameters
    /// - `url`: Resolved request URL attached to the error for diagnostics.
    /// - `message`: Human-readable cancellation reason.
    ///
    /// # Returns
    /// `Some` [`HttpError`] (including method and URL context) when a token
    /// exists and is already cancelled; otherwise `None`.
    pub(crate) fn cancelled_error_if_needed(&self, url: &Url, message: &str) -> Option<HttpError> {
        if self
            .cancellation_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            Some(
                HttpError::cancelled(message.to_string())
                    .with_method(self.method.clone())
                    .with_url(url.clone()),
            )
        } else {
            None
        }
    }

    /// Attaches the correct reqwest body encoding for each [`HttpRequestBody`]
    /// variant.
    ///
    /// # Parameters
    /// - `builder`: Partially configured [`reqwest::RequestBuilder`]
    ///   (method/URL/headers already set).
    /// - `body`: Payload variant to attach; moved into the builder.
    ///
    /// # Returns
    /// The same builder with an appropriate `.body(...)` applied (or unchanged
    /// for [`HttpRequestBody::Empty`]).
    fn apply_request_body(
        builder: reqwest::RequestBuilder,
        body: HttpRequestBody,
    ) -> reqwest::RequestBuilder {
        match body {
            HttpRequestBody::Empty => builder,
            HttpRequestBody::Bytes(bytes)
            | HttpRequestBody::Json(bytes)
            | HttpRequestBody::Form(bytes)
            | HttpRequestBody::Multipart(bytes)
            | HttpRequestBody::Ndjson(bytes) => builder.body(bytes),
            HttpRequestBody::Stream(chunks) => {
                let body_stream = futures_stream::iter(
                    chunks.into_iter().map(Result::<Bytes, std::io::Error>::Ok),
                );
                builder.body(reqwest::Body::wrap_stream(body_stream))
            }
            HttpRequestBody::Text(text) => builder.body(text),
        }
    }
}
