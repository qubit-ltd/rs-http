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

use http::{HeaderMap, HeaderName, HeaderValue, Method};
use qubit_function::MutatingFunction;
use tokio_util::sync::CancellationToken;
use url::Host;
use url::Url;

use crate::{AsyncHeaderInjector, HeaderInjector, HttpError, HttpResult};

use super::http_request_body::HttpRequestBody;
use super::http_request_builder::HttpRequestBuilder;
use super::http_request_retry_override::HttpRequestRetryOverride;
use super::parse_header;

/// Immutable snapshot of a single HTTP call produced by [`crate::HttpRequestBuilder`].
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// HTTP method (GET, POST, …).
    method: Method,
    /// Absolute URL string, or path joined with client `base_url` when not parseable as URL.
    path: String,
    /// Query string parameters as `(name, value)` pairs.
    query: Vec<(String, String)>,
    /// Headers added on top of client defaults and injector output.
    headers: HeaderMap,
    /// Serialized body variant.
    body: HttpRequestBody,
    /// Overrides client-wide request timeout when set; otherwise client default applies.
    request_timeout: Option<Duration>,
    /// Base URL copied from client options, used to resolve relative `path`.
    base_url: Option<Url>,
    /// Whether resolved URLs must avoid IPv6 literal hosts.
    ipv4_only: bool,
    /// Optional cancellation token checked before send and during I/O phases.
    cancellation_token: Option<CancellationToken>,
    /// Per-request retry override (enable/disable/method-policy/Retry-After behavior).
    retry_override: HttpRequestRetryOverride,
    /// Client default headers snapshot captured when this request builder was created.
    default_headers: HeaderMap,
    /// Client sync header injectors snapshot captured when this request builder was created.
    injectors: Vec<HeaderInjector>,
    /// Client async header injectors snapshot captured when this request builder was created.
    async_injectors: Vec<AsyncHeaderInjector>,
}

impl HttpRequest {
    pub(super) fn new(builder: HttpRequestBuilder) -> Self {
        Self {
            method: builder.method,
            path: builder.path,
            query: builder.query,
            headers: builder.headers,
            body: builder.body,
            request_timeout: builder.request_timeout,
            base_url: builder.base_url,
            ipv4_only: builder.ipv4_only,
            cancellation_token: builder.cancellation_token,
            retry_override: builder.retry_override,
            default_headers: builder.default_headers,
            injectors: builder.injectors,
            async_injectors: builder.async_injectors,
        }
    }

    /// Returns request method.
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Sets request method.
    pub fn set_method(&mut self, method: Method) -> &mut Self {
        self.method = method;
        self
    }

    /// Returns request path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Sets request path.
    pub fn set_path(&mut self, path: impl Into<String>) -> &mut Self {
        self.path = path.into();
        self
    }

    /// Returns query parameters.
    pub fn query(&self) -> &[(String, String)] {
        &self.query
    }

    /// Appends one query parameter.
    pub fn add_query_param(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> &mut Self {
        self.query.push((key.into(), value.into()));
        self
    }

    /// Clears all query parameters.
    pub fn clear_query_params(&mut self) -> &mut Self {
        self.query.clear();
        self
    }

    /// Returns request headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Sets one header by name/value strings (validated).
    pub fn set_header(&mut self, name: &str, value: &str) -> Result<&mut Self, HttpError> {
        let (header_name, header_value) = parse_header(name, value)?;
        self.headers.insert(header_name, header_value);
        Ok(self)
    }

    /// Sets one header by typed name/value.
    pub fn set_typed_header(&mut self, name: HeaderName, value: HeaderValue) -> &mut Self {
        self.headers.insert(name, value);
        self
    }

    /// Removes one header by typed name.
    pub fn remove_header(&mut self, name: &HeaderName) -> &mut Self {
        self.headers.remove(name);
        self
    }

    /// Clears all headers.
    pub fn clear_headers(&mut self) -> &mut Self {
        self.headers.clear();
        self
    }

    /// Returns request body.
    pub fn body(&self) -> &HttpRequestBody {
        &self.body
    }

    /// Replaces request body.
    pub fn set_body(&mut self, body: HttpRequestBody) -> &mut Self {
        self.body = body;
        self
    }

    /// Returns per-request timeout.
    pub fn request_timeout(&self) -> Option<Duration> {
        self.request_timeout
    }

    /// Sets per-request timeout.
    pub fn set_request_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.request_timeout = Some(timeout);
        self
    }

    /// Clears per-request timeout so client default applies.
    pub fn clear_request_timeout(&mut self) -> &mut Self {
        self.request_timeout = None;
        self
    }

    /// Returns base URL used for resolving relative paths.
    pub fn base_url(&self) -> Option<&Url> {
        self.base_url.as_ref()
    }

    /// Sets base URL used for resolving relative paths.
    pub fn set_base_url(&mut self, base_url: Url) -> &mut Self {
        self.base_url = Some(base_url);
        self
    }

    /// Clears base URL.
    pub fn clear_base_url(&mut self) -> &mut Self {
        self.base_url = None;
        self
    }

    /// Returns whether IPv4-only URL host validation is enabled.
    pub fn ipv4_only(&self) -> bool {
        self.ipv4_only
    }

    /// Sets whether IPv4-only URL host validation is enabled.
    pub fn set_ipv4_only(&mut self, enabled: bool) -> &mut Self {
        self.ipv4_only = enabled;
        self
    }

    /// Returns optional cancellation token.
    pub fn cancellation_token(&self) -> Option<&CancellationToken> {
        self.cancellation_token.as_ref()
    }

    /// Sets cancellation token.
    pub fn set_cancellation_token(&mut self, token: CancellationToken) -> &mut Self {
        self.cancellation_token = Some(token);
        self
    }

    /// Clears cancellation token.
    pub fn clear_cancellation_token(&mut self) -> &mut Self {
        self.cancellation_token = None;
        self
    }

    /// Returns retry override.
    pub fn retry_override(&self) -> &HttpRequestRetryOverride {
        &self.retry_override
    }

    /// Sets retry override.
    pub fn set_retry_override(&mut self, retry_override: HttpRequestRetryOverride) -> &mut Self {
        self.retry_override = retry_override;
        self
    }

    pub(crate) fn take_body(&mut self) -> HttpRequestBody {
        std::mem::replace(&mut self.body, HttpRequestBody::Empty)
    }

    /// Resolves `self.path` as an absolute URL or joins it with `self.base_url` when relative.
    ///
    /// # Returns
    /// Resolved [`Url`] or [`HttpError::invalid_url`] if resolution/validation fails.
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

    /// Rejects IPv6 literal hosts when `ipv4_only` is enabled.
    fn validate_resolved_url_host(&self, url: &Url) -> Result<(), HttpError> {
        if self.ipv4_only && matches!(url.host(), Some(Host::Ipv6(_))) {
            return Err(HttpError::invalid_url(format!(
                "IPv6 literal host is not allowed when ipv4_only=true: {}",
                url
            )));
        }
        Ok(())
    }

    /// Builds final request headers from client snapshots and request-local headers.
    ///
    /// Merge order (later wins on duplicates):
    /// 1. client default headers snapshot,
    /// 2. sync injector output,
    /// 3. async injector output,
    /// 4. request-local headers.
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

    /// Builds a cancelled error when this request's cancellation token exists and is already cancelled.
    ///
    /// # Parameters
    /// - `url`: Resolved request URL for error context.
    /// - `message`: Cancellation message.
    ///
    /// # Returns
    /// `Some(HttpError)` when cancelled, otherwise `None`.
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
}
