/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Immutable HTTP request object.

use std::future::Future;
use std::sync::RwLock;
use std::time::Duration;

use bytes::Bytes;
use futures_util::stream as futures_stream;
use http::{
    HeaderMap,
    HeaderName,
    HeaderValue,
    Method,
};
use qubit_function::MutatingFunction;
use reqwest::Response;
use tokio_util::sync::CancellationToken;
use url::Host;
use url::Url;

use crate::error::{
    backend_error_mapper::map_reqwest_error,
    ReqwestErrorPhase,
};
use crate::{
    AsyncHttpHeaderInjector,
    HttpError,
    HttpErrorKind,
    HttpHeaderInjector,
    HttpLogger,
    HttpRequestStreamingBody,
    HttpResult,
};

use super::http_request_body::HttpRequestBody;
use super::http_request_builder::HttpRequestBuilder;
use super::http_request_retry_override::HttpRequestRetryOverride;
use super::parse_header;

/// Request execution options (timeouts, cancellation, and retry override).
#[derive(Debug, Clone)]
struct HttpRequestExecutionOptions {
    /// Overrides client-wide request timeout when set; otherwise client default applies.
    request_timeout: Option<Duration>,
    /// Per-request write timeout used during request sending.
    write_timeout: Duration,
    /// Per-request read timeout used during response body reads.
    read_timeout: Duration,
    /// Optional cancellation token checked before send and during I/O phases.
    cancellation_token: Option<CancellationToken>,
    /// Per-request retry override (enable/disable/method-policy/Retry-After behavior).
    retry_override: HttpRequestRetryOverride,
}

/// Request context captured from the originating client.
#[derive(Debug, Clone)]
struct HttpRequestContext {
    /// Base URL copied from client options, used to resolve relative `path`.
    base_url: Option<Url>,
    /// Whether resolved URLs must avoid IPv6 literal hosts.
    ipv4_only: bool,
    /// Client default headers snapshot captured when this request builder was created.
    default_headers: HeaderMap,
    /// Client sync header injectors snapshot captured when this request builder was created.
    injectors: Vec<HttpHeaderInjector>,
    /// Client async header injectors snapshot captured when this request builder was created.
    async_injectors: Vec<AsyncHttpHeaderInjector>,
}

/// Immutable snapshot of a single HTTP call produced by
/// [`crate::HttpRequestBuilder`].
#[derive(Debug)]
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
    /// Deferred per-attempt streaming body factory.
    streaming_body: Option<HttpRequestStreamingBody>,
    /// Lazily maintained cache for the currently resolved URL.
    resolved_url: RwLock<Option<Url>>,
    /// Attempt-scoped cache of merged outbound headers after applying
    /// defaults/injectors/request-local headers.
    effective_headers: Option<HeaderMap>,
    /// Request execution options and runtime controls.
    execution_options: HttpRequestExecutionOptions,
    /// Client-derived context for URL and header resolution.
    context: HttpRequestContext,
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
        let mut request = Self {
            method: builder.method,
            path: builder.path,
            query: builder.query,
            headers: builder.headers,
            body: builder.body,
            streaming_body: builder.streaming_body,
            resolved_url: RwLock::new(None),
            effective_headers: None,
            execution_options: HttpRequestExecutionOptions {
                request_timeout: builder.request_timeout,
                write_timeout: builder.write_timeout,
                read_timeout: builder.read_timeout,
                cancellation_token: builder.cancellation_token,
                retry_override: builder.retry_override,
            },
            context: HttpRequestContext {
                base_url: builder.base_url,
                ipv4_only: builder.ipv4_only,
                default_headers: builder.default_headers,
                injectors: builder.injectors,
                async_injectors: builder.async_injectors,
            },
        };
        request.refresh_resolved_url_cache();
        request
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
    pub fn set_path(&mut self, path: &str) -> &mut Self {
        self.path = path.to_string();
        self.refresh_resolved_url_cache();
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
    pub fn add_query_param(&mut self, key: &str, value: &str) -> &mut Self {
        self.query.push((key.to_string(), value.to_string()));
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
        self.invalidate_effective_headers_cache();
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
        self.invalidate_effective_headers_cache();
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
        self.invalidate_effective_headers_cache();
        self
    }

    /// Clears all request-local headers (defaults and injectors are unaffected
    /// until send).
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn clear_headers(&mut self) -> &mut Self {
        self.headers.clear();
        self.invalidate_effective_headers_cache();
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
        self.streaming_body = None;
        self
    }

    /// Sets deferred streaming upload body factory for this request.
    ///
    /// # Parameters
    /// - `streaming_body`: Deferred body stream factory reused across retries.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn set_streaming_body(&mut self, streaming_body: HttpRequestStreamingBody) -> &mut Self {
        self.streaming_body = Some(streaming_body);
        self.body = HttpRequestBody::Empty;
        self
    }

    /// Returns the per-request total timeout, if any.
    ///
    /// # Returns
    /// `Some(duration)` when a request-specific timeout overrides the client
    /// default; otherwise `None`.
    pub fn request_timeout(&self) -> Option<Duration> {
        self.execution_options.request_timeout
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
        self.execution_options.request_timeout = Some(timeout);
        self
    }

    /// Drops the per-request timeout so the client-wide default applies again.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn clear_request_timeout(&mut self) -> &mut Self {
        self.execution_options.request_timeout = None;
        self
    }

    /// Returns the write-phase timeout used while sending the request.
    pub fn write_timeout(&self) -> Duration {
        self.execution_options.write_timeout
    }

    /// Sets the write-phase timeout used while sending the request.
    pub fn set_write_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.execution_options.write_timeout = timeout;
        self
    }

    /// Returns the read-phase timeout used while reading response body bytes.
    pub fn read_timeout(&self) -> Duration {
        self.execution_options.read_timeout
    }

    /// Sets the read-phase timeout used while reading response body bytes.
    pub fn set_read_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.execution_options.read_timeout = timeout;
        self
    }

    /// Returns the optional base URL used to resolve relative [`Self::path`]
    /// values.
    ///
    /// # Returns
    /// `Some` when a base is configured; `None` when only absolute URLs in
    /// `path` are valid.
    pub fn base_url(&self) -> Option<&Url> {
        self.context.base_url.as_ref()
    }

    /// Sets the base URL used for internal URL resolution when `path` is not
    /// absolute.
    ///
    /// # Parameters
    /// - `base_url`: Root URL to join against relative paths.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn set_base_url(&mut self, base_url: Url) -> &mut Self {
        self.context.base_url = Some(base_url);
        self.refresh_resolved_url_cache();
        self
    }

    /// Removes the configured base URL so relative paths can no longer be
    /// resolved without resetting it.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn clear_base_url(&mut self) -> &mut Self {
        self.context.base_url = None;
        self.refresh_resolved_url_cache();
        self
    }

    /// Returns whether IPv6 literal hosts are rejected after URL resolution.
    ///
    /// # Returns
    /// `true` when a resolved URL whose host is an IPv6 literal must be
    /// rejected with [`HttpError::invalid_url`].
    pub fn ipv4_only(&self) -> bool {
        self.context.ipv4_only
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
        self.context.ipv4_only = enabled;
        self.refresh_resolved_url_cache();
        self
    }

    /// Returns the cooperative cancellation handle, if configured.
    ///
    /// # Returns
    /// `Some` token checked before send and during I/O; `None` when
    /// cancellation is not wired.
    pub fn cancellation_token(&self) -> Option<&CancellationToken> {
        self.execution_options.cancellation_token.as_ref()
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
        self.execution_options.cancellation_token = Some(token);
        self
    }

    /// Removes any cancellation token from this snapshot.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn clear_cancellation_token(&mut self) -> &mut Self {
        self.execution_options.cancellation_token = None;
        self
    }

    /// Returns the per-request retry override applied by the client pipeline.
    ///
    /// # Returns
    /// Borrowed [`HttpRequestRetryOverride`].
    pub fn retry_override(&self) -> &HttpRequestRetryOverride {
        &self.execution_options.retry_override
    }

    /// Replaces the retry override for this single request.
    ///
    /// # Parameters
    /// - `retry_override`: New override policy and knobs.
    ///
    /// # Returns
    /// `self` for method chaining.
    pub fn set_retry_override(&mut self, retry_override: HttpRequestRetryOverride) -> &mut Self {
        self.execution_options.retry_override = retry_override;
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
    /// Centralizes send-attempt preparation and transport wiring:
    /// - invalidates and recomputes effective headers for each attempt;
    /// - emits request TRACE logs via the provided logger;
    /// - applies query/timeout/body wiring plus cooperative cancellation and
    ///   write-timeout handling.
    ///
    /// # Parameters
    /// - `backend`: Shared reqwest client.
    /// - `logger`: Attempt-scoped request logger.
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
        logger: &HttpLogger<'_>,
    ) -> HttpResult<Response> {
        // Effective headers are cached on the request. Each send attempt must
        // invalidate and recompute them so injector output and request mutations
        // are refreshed instead of reusing stale headers from prior attempts.
        self.invalidate_effective_headers_cache();
        let method = self.method().clone();
        let request_url_context = self.resolved_url_with_query().ok();
        let write_timeout = self.execution_options.write_timeout;
        let cancellation_token = self.execution_options.cancellation_token.clone();
        let headers = Self::await_pre_send_future(
            self.effective_headers(),
            write_timeout,
            cancellation_token,
            &method,
            request_url_context.as_ref(),
            "Request cancelled while preparing request",
            format!(
                "Write timeout after {:?} while preparing request",
                write_timeout
            ),
        )
        .await?
        .clone();
        let url = self.resolved_url()?;
        let request_url = self.resolved_url_with_query()?;
        // Log the request after computing effective headers so TRACE logs
        // include the same query string used by the actual send path.
        logger.log_request(self);
        let mut builder = backend.request(method.clone(), url.clone());
        builder = builder.headers(headers);
        if !self.query.is_empty() {
            builder = builder.query(self.query.as_slice());
        }
        if let Some(timeout) = self.execution_options.request_timeout {
            builder = builder.timeout(timeout);
        }
        if let Some(streaming_body) = self.streaming_body.as_ref() {
            let body = Self::await_pre_send_future(
                async { Ok(streaming_body.to_reqwest_body().await) },
                self.execution_options.write_timeout,
                self.execution_options.cancellation_token.clone(),
                &method,
                Some(&request_url),
                "Request cancelled while preparing streaming request body",
                format!(
                    "Write timeout after {:?} while preparing streaming request body",
                    self.execution_options.write_timeout
                ),
            )
            .await?;
            builder = builder.body(body);
        } else {
            builder = Self::apply_request_body(builder, self.take_body());
        }

        let send_future =
            tokio::time::timeout(self.execution_options.write_timeout, builder.send());
        let next = if let Some(token) = self.execution_options.cancellation_token.as_ref() {
            tokio::select! {
                _ = token.cancelled() => {
                    return Err(HttpError::cancelled("Request cancelled while sending")
                        .with_method(&method)
                        .with_url(&request_url));
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
                ReqwestErrorPhase::Send,
                Some(method.clone()),
                Some(request_url.clone()),
            )),
            Err(_) => Err(HttpError::write_timeout(format!(
                "Write timeout after {:?} while sending request",
                self.execution_options.write_timeout
            ))
            .with_method(&method)
            .with_url(&request_url)),
        }
    }

    /// Waits for one asynchronous pre-send preparation step with cancellation and
    /// write-timeout handling.
    ///
    /// # Parameters
    /// - `future`: Preparation future, such as async header injection or streaming
    ///   body factory execution.
    /// - `write_timeout`: Timeout budget reused for send preparation.
    /// - `cancellation_token`: Optional request cancellation token.
    /// - `method`: Request method for error context.
    /// - `request_url`: Optional resolved request URL for error context.
    /// - `cancellation_message`: Message used when cancellation wins.
    /// - `timeout_message`: Message used when timeout wins.
    ///
    /// # Returns
    /// The future output when it completes before cancellation or timeout.
    ///
    /// # Errors
    /// Returns [`HttpErrorKind::Cancelled`] on cancellation,
    /// [`HttpErrorKind::WriteTimeout`] on timeout, or propagates the future's own
    /// error.
    async fn await_pre_send_future<T, F>(
        future: F,
        write_timeout: Duration,
        cancellation_token: Option<CancellationToken>,
        method: &Method,
        request_url: Option<&Url>,
        cancellation_message: &str,
        timeout_message: String,
    ) -> HttpResult<T>
    where
        F: Future<Output = HttpResult<T>>,
    {
        let timed = tokio::time::timeout(write_timeout, future);
        let next = if let Some(token) = cancellation_token.as_ref() {
            tokio::select! {
                _ = token.cancelled() => {
                    return Err(Self::pre_send_cancelled_error(
                        cancellation_message,
                        method,
                        request_url,
                    ));
                }
                result = timed => result,
            }
        } else {
            timed.await
        };

        match next {
            Ok(result) => result,
            Err(_) => Err(Self::pre_send_write_timeout_error(
                timeout_message,
                method,
                request_url,
            )),
        }
    }

    /// Builds a cancellation error for pre-send preparation.
    ///
    /// # Parameters
    /// - `message`: Cancellation message.
    /// - `method`: Request method for context.
    /// - `request_url`: Optional request URL for context.
    ///
    /// # Returns
    /// Cancellation [`HttpError`] with request context attached.
    fn pre_send_cancelled_error(
        message: &str,
        method: &Method,
        request_url: Option<&Url>,
    ) -> HttpError {
        let mut error = HttpError::cancelled(message).with_method(method);
        if let Some(request_url) = request_url {
            error = error.with_url(request_url);
        }
        error
    }

    /// Builds a write-timeout error for pre-send preparation.
    ///
    /// # Parameters
    /// - `message`: Timeout message.
    /// - `method`: Request method for context.
    /// - `request_url`: Optional request URL for context.
    ///
    /// # Returns
    /// Write-timeout [`HttpError`] with request context attached.
    fn pre_send_write_timeout_error(
        message: String,
        method: &Method,
        request_url: Option<&Url>,
    ) -> HttpError {
        let mut error = HttpError::write_timeout(message).with_method(method);
        if let Some(request_url) = request_url {
            error = error.with_url(request_url);
        }
        error
    }

    /// Returns the resolved URL for current request fields, computing and
    /// caching it on demand.
    ///
    /// # Returns
    /// Resolved [`Url`] value (cloned from cache when already computed).
    ///
    /// # Errors
    /// Returns [`HttpError::invalid_url`] when parsing fails, the base URL is
    /// missing for a relative path, joining fails, or [`Self::ipv4_only`]
    /// rejects an IPv6 literal host.
    pub(crate) fn resolved_url(&self) -> Result<Url, HttpError> {
        let cached = match self.resolved_url.read() {
            Ok(guard) => guard.clone(),
            Err(_) => return Err(HttpError::other("Resolved URL cache read lock poisoned")),
        };
        if let Some(url) = cached.as_ref() {
            return Ok(url.clone());
        }
        let resolved = self.compute_resolved_url()?;
        match self.resolved_url.write() {
            Ok(mut guard) => *guard = Some(resolved.clone()),
            Err(_) => return Err(HttpError::other("Resolved URL cache write lock poisoned")),
        }
        Ok(resolved)
    }

    /// Returns the resolved URL plus request-builder query parameters.
    ///
    /// This mirrors the URL sent by [`Self::send_impl`]: query pairs already
    /// present in [`Self::path`] are preserved, and pairs from
    /// [`Self::query`] are appended in insertion order.
    ///
    /// # Returns
    /// Resolved [`Url`] including query parameters from this request snapshot.
    ///
    /// # Errors
    /// Propagates [`Self::resolved_url`] errors for invalid or unresolved URLs.
    pub(crate) fn resolved_url_with_query(&self) -> Result<Url, HttpError> {
        let mut url = self.resolved_url()?;
        if !self.query.is_empty() {
            {
                let mut pairs = url.query_pairs_mut();
                for (key, value) in &self.query {
                    pairs.append_pair(key, value);
                }
            }
        }
        Ok(url)
    }

    /// Returns cached resolved URL when available.
    pub fn resolved_url_cached(&self) -> Option<Url> {
        self.resolved_url
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Recomputes and stores the current resolved URL.
    fn refresh_resolved_url_cache(&mut self) {
        if let Ok(mut guard) = self.resolved_url.write() {
            *guard = self.compute_resolved_url().ok();
        }
    }

    /// Computes the resolved URL from current path/base/ipv4 settings.
    fn compute_resolved_url(&self) -> Result<Url, HttpError> {
        if let Ok(url) = Url::parse(&self.path) {
            self.validate_resolved_url_host(&url)?;
            return Ok(url);
        }

        let base = self.context.base_url.as_ref().ok_or_else(|| {
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
        if self.context.ipv4_only && matches!(url.host(), Some(Host::Ipv6(_))) {
            return Err(HttpError::invalid_url(format!(
                "IPv6 literal host is not allowed when ipv4_only=true: {}",
                url
            )));
        }
        Ok(())
    }

    /// Returns the attempt-scoped merged outbound headers.
    ///
    /// On first call after invalidation, this computes merged headers by
    /// replaying defaults/injectors/request-local headers and stores them in
    /// [`Self::effective_headers`]. Later calls in the same attempt return the
    /// cached map.
    ///
    /// Why this API is async:
    /// - async injectors are part of header assembly and may perform awaitable
    ///   work (for example token refresh or other I/O-backed value resolution).
    /// - therefore header materialization cannot be fully synchronous.
    ///
    /// Merge order (later wins on duplicates):
    /// 1. Client default headers snapshot captured when the builder was
    ///    created.
    /// 2. Synchronous injector output in registration order.
    /// 3. Asynchronous injector output in registration order.
    /// 4. Request-local headers from this snapshot.
    ///
    /// # Returns
    /// Borrowed merged [`HeaderMap`] from the cache.
    ///
    /// # Errors
    /// Propagates failures returned by any injector's `apply` implementation.
    pub(crate) async fn effective_headers(&mut self) -> HttpResult<&HeaderMap> {
        if self.effective_headers.is_none() {
            self.effective_headers = Some(self.compute_effective_headers().await?);
        }
        Ok(self
            .effective_headers
            .as_ref()
            .expect("effective headers cache must be populated after computation"))
    }

    /// Returns cached merged outbound headers when available.
    pub(crate) fn effective_headers_cached(&self) -> Option<&HeaderMap> {
        self.effective_headers.as_ref()
    }

    /// Clears the effective-header cache.
    ///
    /// This method invalidates [`Self::effective_headers`] so the next call to
    /// [`Self::effective_headers`] recomputes merged headers by re-running
    /// defaults and header injectors.
    ///
    /// Why this is needed:
    /// - request-local headers may have been mutated (`set_header`, `clear_headers`, etc.);
    /// - injector output may be time-sensitive (for example rotating auth token
    ///   or timestamp-based signatures), so each send attempt should recompute
    ///   merged headers instead of reusing stale values from prior attempts.
    ///
    /// When to call:
    /// - immediately before starting a new send attempt;
    /// - after any mutation that can change final outbound headers.
    pub(crate) fn invalidate_effective_headers_cache(&mut self) {
        self.effective_headers = None;
    }

    /// Computes merged outbound headers without touching the cache.
    async fn compute_effective_headers(&self) -> HttpResult<HeaderMap> {
        let mut headers = self.context.default_headers.clone();

        for injector in &self.context.injectors {
            injector.apply(&mut headers)?;
        }
        for injector in &self.context.async_injectors {
            injector.apply(&mut headers).await?;
        }

        headers.extend(self.headers.clone());
        Ok(headers)
    }

    /// Returns a pre-cancelled [`HttpError`] when a token is present and
    /// already cancelled.
    ///
    /// # Parameters
    /// - `message`: Human-readable cancellation reason.
    ///
    /// # Returns
    /// `Some` [`HttpError`] (including method context and cached URL when
    /// available) when a token exists and is already cancelled; otherwise
    /// `None`.
    pub(crate) fn cancelled_error_if_needed(&self, message: &str) -> Option<HttpError> {
        if self
            .execution_options
            .cancellation_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            let mut error = HttpError::cancelled(message.to_string()).with_method(&self.method);
            if let Ok(url) = self.resolved_url_with_query() {
                error = error.with_url(&url);
            }
            Some(error)
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

impl Clone for HttpRequest {
    fn clone(&self) -> Self {
        Self {
            method: self.method.clone(),
            path: self.path.clone(),
            query: self.query.clone(),
            headers: self.headers.clone(),
            body: self.body.clone(),
            streaming_body: self.streaming_body.clone(),
            resolved_url: RwLock::new(self.resolved_url_cached()),
            effective_headers: self.effective_headers.clone(),
            execution_options: self.execution_options.clone(),
            context: self.context.clone(),
        }
    }
}
