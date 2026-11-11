// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Builder for [`super::http_request::HttpRequest`].

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use http::header::CONTENT_TYPE;
use qubit_budget::json::JsonEncodeLimits;
use qubit_budget::json::JsonEncodeSession;
use qubit_json::encode::JsonEncoder;
use qubit_redact::Redactor;
use serde::Serialize;
use url::Url;
use url::form_urlencoded;

use super::http_request::HttpRequest;
use super::http_request_body::HttpRequestBody;
use super::http_request_retry_override::HttpRequestRetryOverride;
use super::parse_header;
use super::validate_positive_timeout;
use super::validate_response_header_timeout;
use crate::AsyncHttpHeaderInjector;
use crate::HttpCancellationToken;
use crate::HttpClient;
use crate::HttpError;
use crate::HttpHeaderInjector;
use crate::HttpOriginPolicy;
use crate::HttpRequestBodyByteStream;
use crate::HttpRequestStreamingBody;
use crate::HttpResult;
use crate::HttpRetryMethodPolicy;
use crate::content_type;

/// Builder for [`HttpRequest`](super::http_request::HttpRequest).
///
/// # Examples
///
/// ```
/// use http::Method;
/// use qubit_http::HttpClient;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = HttpClient::builder().build()?;
/// let request = client
///     .request(Method::GET, "https://example.com/items")
///     .query_param("page", "2")
///     .build();
/// assert_eq!(request.resolved_url()?.as_str(), "https://example.com/items?page=2");
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
#[must_use = "request builder configuration must be used to build a request"]
pub struct HttpRequestBuilder {
    /// HTTP method (e.g. GET, POST).
    pub(super) method: Method,
    /// Request path without the query string.
    pub(super) path: String,
    /// Query parameters as `(key, value)` pairs, appended to the URL when
    /// built.
    pub(super) query: Vec<(String, String)>,
    /// Request headers.
    pub(super) headers: HeaderMap,
    /// Whether the current Content-Type was inferred from a body helper.
    auto_content_type: bool,
    /// Request body; empty if not set.
    pub(super) body: HttpRequestBody,
    /// Deferred streaming upload body factory for per-attempt stream creation.
    pub(super) streaming_body: Option<HttpRequestStreamingBody>,
    /// Per-request timeout; if unset, the client default applies.
    pub(super) request_timeout: Option<Duration>,
    /// Per-request response-header timeout covering asynchronous preparation
    /// and sending.
    pub(super) response_header_timeout: Duration,
    /// Per-request read timeout used by buffered/stream response reading.
    pub(super) read_timeout: Duration,
    /// Base URL copied from client options and used by
    /// [`HttpRequest::resolved_url`].
    pub(super) base_url: Option<Url>,
    /// Policy used to validate the relationship between the request URL and
    /// its configured base URL.
    pub(super) origin_policy: HttpOriginPolicy,
    /// Whether IPv6 literal hosts are rejected during URL resolution.
    pub(super) ipv4_only: bool,
    /// Optional cancellation token for this request.
    pub(super) cancellation_token: Option<HttpCancellationToken>,
    /// Per-request retry override for one-off retry behavior customization.
    pub(super) retry_override: HttpRequestRetryOverride,
    /// Default headers snapshot from the originating client.
    pub(super) default_headers: HeaderMap,
    /// Sync header injectors snapshot from the originating client.
    pub(super) injectors: Vec<HttpHeaderInjector>,
    /// Async header injectors snapshot from the originating client.
    pub(super) async_injectors: Vec<AsyncHttpHeaderInjector>,
    /// Log redaction snapshot from the originating client.
    pub(super) log_redactor: Redactor,
    /// JSON output limits snapshot from the originating client.
    pub(super) json_encode_limits: JsonEncodeLimits,
}

/// Formats the builder with sensitive URL and header values redacted.
impl fmt::Debug for HttpRequestBuilder {
    /// Formats builder state while applying the captured redaction policy.
    ///
    /// # Parameters
    /// - `formatter`: Destination formatter.
    ///
    /// # Returns
    /// Result from writing the redacted builder representation.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut batch = self.log_redactor.diagnostic_batch();
        let url = self.debug_resolved_url().map(|url| batch.redact_http_url(url.as_str()));
        let base_url = self.base_url.as_ref().map(|url| batch.redact_http_url(url.as_str()));
        let headers = batch.redact_http_headers(&self.headers);
        let default_headers = batch.redact_http_headers(&self.default_headers);
        let output = batch.finish();
        let url = url.map(|handle| output.text(handle));
        let base_url = base_url.map(|handle| output.text(handle));
        let headers = output.text(headers);
        let default_headers = output.text(default_headers);
        formatter
            .debug_struct("HttpRequestBuilder")
            .field("method", &self.method)
            .field("url", &url)
            .field("headers", &headers)
            .field("body", &self.body)
            .field("streaming_body", &self.streaming_body.as_ref().map(|_| "present"))
            .field("request_timeout", &self.request_timeout)
            .field("response_header_timeout", &self.response_header_timeout)
            .field("read_timeout", &self.read_timeout)
            .field("base_url", &base_url)
            .field("ipv4_only", &self.ipv4_only)
            .field("cancellation_token_present", &self.cancellation_token.is_some())
            .field("retry_override", &self.retry_override)
            .field("default_headers", &default_headers)
            .field("injector_count", &self.injectors.len())
            .field("async_injector_count", &self.async_injectors.len())
            .field("json_encode_limits", &self.json_encode_limits)
            .finish()
    }
}

impl HttpRequestBuilder {
    /// Starts a builder with method/path and copies supported defaults from
    /// client options.
    ///
    /// # Parameters
    /// - `method`: HTTP verb.
    /// - `path`: URL or relative path string.
    /// - `client`: Source client whose relevant defaults are copied into this
    ///   builder.
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
            auto_content_type: false,
            body: HttpRequestBody::Empty,
            streaming_body: None,
            request_timeout: options.timeouts.request_timeout,
            response_header_timeout: options.timeouts.response_header_timeout,
            read_timeout: options.timeouts.read_timeout,
            base_url: options.base_url.clone(),
            origin_policy: options.origin_policy,
            ipv4_only: options.ipv4_only,
            cancellation_token: None,
            retry_override: HttpRequestRetryOverride::default(),
            default_headers: client.headers_snapshot(),
            injectors: client.injectors_snapshot(),
            async_injectors: client.async_injectors_snapshot(),
            log_redactor: client.log_redactor().clone(),
            json_encode_limits: options.json_encode_limits,
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
    ///
    /// # Type Parameters
    /// - `'a`: Lifetime of the borrowed query keys and values.
    /// - `I`: Iterator producing query key/value pairs.
    pub fn query_params<'a, I>(mut self, params: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        for (key, value) in params {
            self = self.query_param(key, value);
        }
        self
    }

    /// Validates and inserts one explicit header.
    ///
    /// An explicit `Content-Type` is preserved by later body helpers, even
    /// when its value matches a type that a helper would generate.
    ///
    /// # Parameters
    /// - `name`: Header name (must be valid [`http::header::HeaderName`]
    ///   bytes).
    /// - `value`: Header value (must be valid [`http::header::HeaderValue`]).
    ///
    /// # Returns
    /// `Ok(self)` or [`HttpError`] if name/value are invalid.
    pub fn header(mut self, name: &str, value: &str) -> HttpResult<Self> {
        let (header_name, header_value) =
            parse_header(name, value).map_err(|error| Self::attach_log_redactor(error, &self.log_redactor))?;
        let is_content_type = header_name == CONTENT_TYPE;
        self.headers.insert(header_name, header_value);
        if is_content_type {
            self.auto_content_type = false;
        }
        Ok(self)
    }

    /// Merges explicit headers into this builder by header name.
    ///
    /// Each name present in the input replaces all existing values for that
    /// name. Multiple values in the input map are preserved, and existing
    /// headers absent from the input remain unchanged. A map containing
    /// `Content-Type` makes that value explicit, so later body helpers preserve
    /// it.
    ///
    /// # Parameters
    /// - `headers`: Header map to merge using replacement semantics.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        if headers.contains_key(CONTENT_TYPE) {
            self.auto_content_type = false;
        }
        self.headers.extend(headers);
        self
    }

    /// Sets the body to raw bytes without inferring a media type.
    ///
    /// A `Content-Type` generated by an earlier body helper is removed. An
    /// explicitly supplied value is preserved, and client defaults or
    /// injectors can apply when the request is executed.
    ///
    /// # Parameters
    /// - `body`: Payload.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn bytes_body(mut self, body: impl Into<Bytes>) -> Self {
        self.replace_auto_content_type(None);
        self.body = HttpRequestBody::Bytes(body.into());
        self.streaming_body = None;
        self
    }

    /// Sets the body to an ordered chunk stream for incremental upload.
    ///
    /// # Parameters
    /// - `chunks`: Iterator of chunks in send order.
    ///
    /// The iterator is eagerly collected into memory during this call and must
    /// be finite. Memory usage grows with the total payload. Use
    /// [`Self::streaming_body`] for a deferred per-attempt stream factory.
    /// Although the chunks are collected in memory, this request is represented
    /// as a streaming body by reqwest. The body is not replayed across an
    /// internal `307` or `308` redirect; the redirect status is returned unless
    /// an outer retry policy retries it. Prefer the final upload URL, or use
    /// [`Self::bytes_body`] when the payload fits the application's memory
    /// budget.
    ///
    /// A `Content-Type` generated by an earlier body helper is removed; an
    /// explicitly supplied value is preserved.
    ///
    /// # Type Parameters
    /// - `I`: Iterator producing chunks in send order.
    /// - `B`: Chunk value convertible to `Bytes`.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn stream_body<I, B>(mut self, chunks: I) -> Self
    where
        I: IntoIterator<Item = B>,
        B: Into<Bytes>,
    {
        self.replace_auto_content_type(None);
        self.body = HttpRequestBody::Stream(chunks.into_iter().map(Into::into).collect());
        self.streaming_body = None;
        self
    }

    /// Sets a deferred streaming upload body factory.
    ///
    /// The factory runs once per send attempt and returns a fresh async byte
    /// stream, which allows outer retries to rebuild the outbound stream body.
    /// It is not called again for an internal redirect: streaming bodies are
    /// not replayed through `307` or `308`, which are returned as status errors
    /// unless an outer retry policy retries them. Prefer the final upload URL,
    /// or use [`Self::bytes_body`] for payloads that fit the application's
    /// memory budget.
    ///
    /// A `Content-Type` generated by an earlier body helper is removed; an
    /// explicitly supplied value is preserved.
    ///
    /// # Parameters
    /// - `factory`: Async stream factory for per-attempt body generation.
    ///
    /// # Type Parameters
    /// - `F`: Thread-safe closure that creates the async body stream.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn streaming_body<F>(mut self, factory: F) -> Self
    where
        F: Fn() -> Pin<Box<dyn Future<Output = HttpRequestBodyByteStream> + Send + 'static>> + Send + Sync + 'static,
    {
        self.replace_auto_content_type(None);
        self.streaming_body = Some(HttpRequestStreamingBody::new(factory));
        self.body = HttpRequestBody::Empty;
        self
    }

    /// Sets a UTF-8 text body and sets the inferred
    /// `text/plain; charset=utf-8` value.
    ///
    /// This replaces a `Content-Type` inferred by an earlier body helper. An
    /// explicitly supplied value is preserved.
    ///
    /// # Parameters
    /// - `body`: Text payload.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn text_body(mut self, body: impl Into<String>) -> Self {
        self.replace_auto_content_type(Some(HeaderValue::from_static("text/plain; charset=utf-8")));
        self.body = HttpRequestBody::Text(body.into());
        self.streaming_body = None;
        self
    }

    /// Serializes `value` to JSON, sets body to those bytes, and infers
    /// `application/json` unless an explicit `Content-Type` is present.
    ///
    /// This replaces a `Content-Type` inferred by an earlier body helper.
    ///
    /// # Parameters
    /// - `value`: Serializable value.
    ///
    /// # Returns
    /// `Ok(self)` or [`HttpError`] if JSON encoding exceeds the client default
    /// limits or fails.
    ///
    /// # Type Parameters
    /// - `T`: Serializable JSON value type.
    pub fn json_body<T>(self, value: &T) -> HttpResult<Self>
    where
        T: Serialize,
    {
        let limits = self.json_encode_limits;
        self.json_body_with_limits(value, limits)
    }

    /// Serializes `value` to JSON with explicit resource limits, sets body to
    /// those bytes, and infers `application/json` unless an explicit
    /// `Content-Type` is present. This replaces a value inferred by an earlier
    /// body helper.
    ///
    /// # Parameters
    /// - `value`: Serializable value.
    /// - `limits`: Per-request encoded-value and output-byte limits.
    ///
    /// # Returns
    /// `Ok(self)` or [`HttpError`] if JSON encoding exceeds a limit or fails.
    ///
    /// # Type Parameters
    /// - `T`: Serializable JSON value type.
    pub fn json_body_with_limits<T>(mut self, value: &T, limits: JsonEncodeLimits) -> HttpResult<Self>
    where
        T: Serialize,
    {
        let bytes = JsonEncoder::with_limits(limits).to_vec(value).map_err(|error| {
            Self::attach_log_redactor(
                HttpError::decode("Failed to encode JSON body").with_source(error),
                &self.log_redactor,
            )
        })?;
        self.replace_auto_content_type(Some(HeaderValue::from_static("application/json")));
        self.body = HttpRequestBody::Json(Bytes::from(bytes));
        self.streaming_body = None;
        Ok(self)
    }

    /// Serializes key-value pairs as `application/x-www-form-urlencoded`.
    ///
    /// This replaces a `Content-Type` inferred by an earlier body helper. An
    /// explicitly supplied value is preserved.
    ///
    /// # Parameters
    /// - `fields`: Iterable of `(key, value)` string pairs.
    ///
    /// # Type Parameters
    /// - `I`: Iterator yielding borrowed key/value pairs.
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
        self.replace_auto_content_type(Some(HeaderValue::from_static("application/x-www-form-urlencoded")));
        self.body = HttpRequestBody::Form(Bytes::from(body));
        self.streaming_body = None;
        self
    }

    /// Sets multipart body bytes and ensures Content-Type boundary consistency.
    ///
    /// A boundary generated by an earlier multipart helper is replaced. An
    /// explicit `Content-Type` is validated against `boundary`; a missing
    /// boundary parameter is added while other explicit parameters are kept.
    ///
    /// # Parameters
    /// - `body`: Multipart payload bytes.
    /// - `boundary`: Token-safe multipart boundary used in payload framing.
    ///
    /// # Returns
    /// `Ok(self)` for chaining.
    ///
    /// # Errors
    /// Returns [`HttpError`] when `boundary` is not a 1 to 70 character
    /// ASCII boundary from the unquoted MIME-safe subset, or when an existing
    /// `Content-Type` is not UTF-8 multipart content with a valid matching
    /// boundary.
    pub fn multipart_body(mut self, body: impl Into<Bytes>, boundary: &str) -> HttpResult<Self> {
        if !content_type::is_valid_multipart_boundary(boundary) {
            return Err(Self::attach_log_redactor(
                HttpError::other(
                    "Invalid multipart boundary for multipart_body: expected 1 to 70 unquoted MIME-safe ASCII characters",
                ),
                &self.log_redactor,
            ));
        }
        if self.auto_content_type {
            let value = HeaderValue::from_str(&format!("multipart/form-data; boundary={boundary}"))
                .expect("validated multipart boundary should build a valid Content-Type");
            self.headers.insert(CONTENT_TYPE, value);
        } else if let Some(existing) = self.headers.get(CONTENT_TYPE) {
            let existing = existing.to_str().map_err(|error| {
                Self::attach_log_redactor(
                    HttpError::other(format!("Existing multipart Content-Type must be valid UTF-8: {error}")),
                    &self.log_redactor,
                )
            })?;
            if !content_type::is_multipart(existing) {
                return Err(Self::attach_log_redactor(
                    HttpError::other("Existing Content-Type must be multipart when using multipart_body"),
                    &self.log_redactor,
                ));
            }
            let existing_boundary = content_type::unique_parameter(existing, "boundary").map_err(|_| {
                Self::attach_log_redactor(
                    HttpError::other("Existing multipart Content-Type boundary is malformed or invalid"),
                    &self.log_redactor,
                )
            })?;
            if let Some(existing_boundary) = existing_boundary {
                if !content_type::is_valid_multipart_boundary(&existing_boundary) {
                    return Err(Self::attach_log_redactor(
                        HttpError::other("Existing multipart Content-Type boundary is malformed or invalid"),
                        &self.log_redactor,
                    ));
                }
                if existing_boundary != boundary {
                    return Err(Self::attach_log_redactor(
                        HttpError::other(format!(
                            "Existing multipart Content-Type boundary '{existing_boundary}' does not match multipart_body boundary '{boundary}'"
                        )),
                        &self.log_redactor,
                    ));
                }
            } else {
                let value = existing.trim().trim_end_matches(';').trim_end();
                let value = HeaderValue::from_str(&format!("{value}; boundary={boundary}"))
                    .expect("validated multipart boundary should build a valid Content-Type");
                self.headers.insert(CONTENT_TYPE, value);
            }
            self.auto_content_type = false;
        } else {
            let value = HeaderValue::from_str(&format!("multipart/form-data; boundary={boundary}"))
                .expect("validated multipart boundary should build a valid Content-Type");
            self.headers.insert(CONTENT_TYPE, value);
            self.auto_content_type = true;
        }
        self.body = HttpRequestBody::Multipart(body.into());
        self.streaming_body = None;
        Ok(self)
    }

    /// Serializes records as NDJSON (`one JSON object per line`).
    ///
    /// This sets or replaces the inferred `application/x-ndjson` value. An
    /// explicitly supplied `Content-Type` is preserved.
    ///
    /// # Parameters
    /// - `records`: Serializable records to encode as NDJSON lines.
    ///
    /// # Returns
    /// `Ok(self)` for chaining.
    ///
    /// # Errors
    /// Returns [`HttpError`] when the client default output limit is exceeded
    /// or any record fails JSON serialization or resource admission.
    ///
    /// # Type Parameters
    /// - `T`: Serializable record type.
    pub fn ndjson_body<T>(self, records: &[T]) -> HttpResult<Self>
    where
        T: Serialize,
    {
        let limits = self.json_encode_limits;
        self.ndjson_body_with_limits(records, limits)
    }

    /// Serializes records as NDJSON with explicit resource limits.
    ///
    /// The output-byte bound includes the newline after every record and is
    /// shared by the complete record sequence.
    /// The inferred `Content-Type` is set or replaced after serialization
    /// succeeds; an explicitly supplied value is preserved.
    ///
    /// # Parameters
    /// - `records`: Serializable records to encode as NDJSON lines.
    /// - `limits`: Per-request encoded-value and aggregate output-byte limits.
    ///
    /// # Returns
    /// `Ok(self)` for chaining.
    ///
    /// # Errors
    /// Returns [`HttpError`] when line terminators cannot fit the output limit
    /// or any record fails JSON serialization or resource admission.
    ///
    /// # Type Parameters
    /// - `T`: Serializable record type.
    pub fn ndjson_body_with_limits<T>(mut self, records: &[T], limits: JsonEncodeLimits) -> HttpResult<Self>
    where
        T: Serialize,
    {
        let newline_bytes = records.len();
        let limits = match limits.max_output_bytes() {
            Some(maximum) => {
                let Some(record_output_bytes) = maximum.checked_sub(newline_bytes) else {
                    return Err(Self::attach_log_redactor(
                        HttpError::decode(
                            "Failed to encode NDJSON body: output budget is smaller than required line terminators",
                        ),
                        &self.log_redactor,
                    ));
                };
                limits.into_builder().max_output_bytes(record_output_bytes).build()
            }
            None => limits,
        };
        let mut encoder = JsonEncoder::new(JsonEncodeSession::from_limits(limits));
        let mut payload = Vec::new();
        for record in records {
            let line = encoder.to_vec(record).map_err(|error| {
                Self::attach_log_redactor(
                    HttpError::decode("Failed to encode NDJSON record").with_source(error),
                    &self.log_redactor,
                )
            })?;
            payload.extend_from_slice(&line);
            payload.push(b'\n');
        }
        self.replace_auto_content_type(Some(HeaderValue::from_static("application/x-ndjson")));
        self.body = HttpRequestBody::Ndjson(Bytes::from(payload));
        self.streaming_body = None;
        Ok(self)
    }

    /// Sets an optional reqwest per-attempt network request and response-body
    /// deadline, overriding the client default. It excludes asynchronous header
    /// injection, streaming-body factory preparation, retry waits, and the
    /// duration of other attempts. Use `response_header_timeout` to bound
    /// asynchronous preparation through receipt of response headers.
    ///
    /// # Parameters
    /// - `timeout`: The reqwest per-attempt network request and response-body
    ///   deadline.
    ///
    /// # Returns
    /// `Ok(self)` for chaining.
    ///
    /// # Errors
    /// Returns [`HttpError`] when `timeout` is zero.
    pub fn request_timeout(mut self, timeout: Duration) -> HttpResult<Self> {
        validate_positive_timeout("request_timeout", timeout)
            .map_err(|error| Self::attach_log_redactor(error, &self.log_redactor))?;
        self.request_timeout = Some(timeout);
        Ok(self)
    }

    /// Attaches the request's captured redaction policy to a builder error.
    ///
    /// # Parameters
    /// - `error`: Error to annotate for safe diagnostics.
    /// - `log_redactor`: Request-captured redaction policy.
    fn attach_log_redactor(error: HttpError, log_redactor: &Redactor) -> HttpError {
        error.with_log_redactor(log_redactor.clone())
    }

    /// Replaces an inferred request-local media type without overriding an
    /// explicitly configured value.
    ///
    /// # Parameters
    /// - `value`: New inferred value, or `None` to remove it.
    fn replace_auto_content_type(&mut self, value: Option<HeaderValue>) {
        if !self.auto_content_type && self.headers.contains_key(CONTENT_TYPE) {
            return;
        }
        self.headers.remove(CONTENT_TYPE);
        self.auto_content_type = value.is_some();
        if let Some(value) = value {
            self.headers.insert(CONTENT_TYPE, value);
        }
    }

    /// Overrides the asynchronous preparation-to-response-header deadline for
    /// this request.
    ///
    /// # Parameters
    /// - `timeout`: Maximum time from asynchronous request preparation through
    ///   response headers.
    ///
    /// # Returns
    /// `Ok(self)` for chaining.
    ///
    /// # Errors
    /// Returns [`HttpError`] when `timeout` is zero or cannot be represented
    /// as a platform deadline.
    pub fn response_header_timeout(mut self, timeout: Duration) -> HttpResult<Self> {
        validate_response_header_timeout(timeout)
            .map_err(|error| Self::attach_log_redactor(error, &self.log_redactor))?;
        self.response_header_timeout = timeout;
        Ok(self)
    }

    /// Bounds each normal response-body read. For non-success status responses,
    /// it also bounds complete error-body capture from when capture begins;
    /// expiry retains the status and partial raw body marked incomplete.
    ///
    /// # Parameters
    /// - `timeout`: Maximum duration allowed for each normal body read and the
    ///   complete non-success error-body capture phase.
    ///
    /// # Returns
    /// `Ok(self)` for chaining.
    ///
    /// # Errors
    /// Returns [`HttpError`] when `timeout` is zero.
    pub fn read_timeout(mut self, timeout: Duration) -> HttpResult<Self> {
        validate_positive_timeout("read_timeout", timeout)
            .map_err(|error| Self::attach_log_redactor(error, &self.log_redactor))?;
        self.read_timeout = timeout;
        Ok(self)
    }

    /// Overrides the client default base URL for this request.
    ///
    /// # Parameters
    /// - `base_url`: Base URL used when resolving relative request paths.
    ///
    /// # Returns
    /// `self` for chaining.
    #[inline]
    pub fn base_url(mut self, base_url: Url) -> Self {
        self.base_url = Some(base_url);
        self
    }

    /// Clears the base URL for this request.
    ///
    /// # Returns
    /// `self` for chaining.
    #[inline]
    pub fn clear_base_url(mut self) -> Self {
        self.base_url = None;
        self
    }

    /// Overrides whether this request enforces IPv4-only literal-host
    /// validation.
    ///
    /// # Parameters
    /// - `enabled`: `true` to reject IPv6 literal hosts, `false` to allow them.
    ///
    /// # Returns
    /// `self` for chaining.
    #[inline]
    pub fn ipv4_only(mut self, enabled: bool) -> Self {
        self.ipv4_only = enabled;
        self
    }

    /// Binds a [`HttpCancellationToken`] to this request.
    ///
    /// # Parameters
    /// - `token`: Cancellation token checked before send and during
    ///   request/stream I/O.
    ///
    /// # Returns
    /// `self` for chaining.
    #[inline]
    pub fn cancellation_token(mut self, token: HttpCancellationToken) -> Self {
        self.cancellation_token = Some(token);
        self
    }

    /// Forces retry enabled for this request even if client-level retry is
    /// disabled.
    ///
    /// This only overrides whether retry is enabled. The effective method
    /// policy and `max_attempts` still apply. For POST/PATCH under the default
    /// method policy, also configure
    /// `retry_method_policy(HttpRetryMethodPolicy::AllMethods)` when replay is
    /// safe. Retrying requires `max_attempts` greater than one.
    ///
    /// # Returns
    /// `self` for chaining.
    #[inline]
    pub fn force_retry(mut self) -> Self {
        self.retry_override = self.retry_override.force_enable();
        self
    }

    /// Disables retry for this request even if client-level retry is enabled.
    ///
    /// # Returns
    /// `self` for chaining.
    #[inline]
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
    #[inline]
    pub fn retry_method_policy(mut self, policy: HttpRetryMethodPolicy) -> Self {
        self.retry_override = self.retry_override.with_method_policy(policy);
        self
    }

    /// Enables or disables honoring `Retry-After` for this request.
    ///
    /// # Parameters
    /// - `enabled`: `true` to honor `Retry-After` on retryable status responses
    ///   (`429` and `5xx`).
    ///
    /// # Returns
    /// `self` for chaining.
    #[inline]
    pub fn honor_retry_after(mut self, enabled: bool) -> Self {
        self.retry_override = self.retry_override.with_honor_retry_after(enabled);
        self
    }

    /// Consumes the builder into a mutable [`HttpRequest`] snapshot.
    ///
    /// # Returns
    /// Built [`HttpRequest`].
    pub fn build(self) -> HttpRequest {
        HttpRequest::new(self)
    }

    /// Returns the URL this builder would send if it can be resolved now.
    ///
    /// # Returns
    /// Resolved URL including builder query pairs, or `None` when unresolved.
    fn debug_resolved_url(&self) -> Option<Url> {
        let mut url = match Url::parse(&self.path) {
            Ok(url) => url,
            Err(_) => self.base_url.as_ref()?.join(&self.path).ok()?,
        };
        if !self.query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in &self.query {
                pairs.append_pair(key, value);
            }
        }
        Some(url)
    }
}
