// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unified HTTP response type and helpers.
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use bytes::Bytes;
use bytes::BytesMut;
use futures_util::StreamExt;
use futures_util::stream as futures_stream;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use http::StatusCode;
use http::header::CONTENT_LENGTH;
use http::header::CONTENT_TYPE;
use qubit_budget::ResourceBudget;
use qubit_budget::json::JsonValueLimits;
use qubit_json::decode::JsonDecoder;
use qubit_redact::Redactor;
use qubit_redact::formats::http::BodyCapture;
use serde::de::DeserializeOwned;
use tokio::time::Instant;
use url::Url;

use super::HttpResponseMeta;
use super::HttpResponseOptions;
use super::internal::HttpResponseBodyState;
use super::internal::http_error_body_capture::HttpErrorBodyCapture;
use super::internal::http_response_runtime::HttpResponseRuntime;
use super::internal::http_response_runtime::previous_body_read_error;
use super::internal::http_response_runtime::remember_body_read_failure_state;
use crate::HttpByteStream;
use crate::HttpCancellationToken;
use crate::HttpError;
use crate::HttpErrorKind;
use crate::HttpResult;
use crate::content_type;
use crate::error::ReqwestErrorPhase;
use crate::error::backend_error_mapper::map_reqwest_error;
use crate::json_limits::json_decode_limits;
use crate::sse::DoneMarkerPolicy;
use crate::sse::SseChunkStream;
use crate::sse::SseCompletionPolicy;
use crate::sse::SseJsonMode;
use crate::sse::SseMessageStream;

/// Maps one backend response body read error into this crate's error model.
///
/// # Parameters
/// - `error`: Backend error returned while reading response body bytes.
/// - `method`: Request method used for diagnostics.
/// - `url`: Request URL used for diagnostics.
/// - `status`: Response status attached to the resulting diagnostic error.
/// - `log_redactor`: Policy snapshot used to render error context.
///
/// # Returns
/// [`HttpErrorKind::ReadTimeout`] for timeout errors, otherwise
/// [`HttpErrorKind::Transport`] for backend body read failures.
fn map_response_read_error(
    error: reqwest::Error,
    method: Method,
    url: Url,
    status: StatusCode,
    log_redactor: &Redactor,
) -> HttpError {
    if error.is_timeout() {
        return map_reqwest_error(error, HttpErrorKind::Transport, ReqwestErrorPhase::Read, method, url)
            .with_status(status)
            .with_log_redactor(log_redactor.clone());
    }

    let error = error.without_url();
    HttpError::transport(format!("Failed to read response body: {}", error))
        .with_method(&method)
        .with_url(&url)
        .with_status(status)
        .with_source(error)
        .with_log_redactor(log_redactor.clone())
}

/// Parses the original response body length hint from backend metadata.
///
/// # Parameters
/// - `method`: Request method associated with the response.
/// - `status`: Response status code.
/// - `headers`: Response headers that may contain `Content-Length`.
///
/// # Returns
/// The advertised body length, or `None` when it is unavailable or invalid.
fn parse_content_length_hint(method: &Method, status: StatusCode, headers: &HeaderMap) -> Option<u64> {
    if method == Method::HEAD || status == StatusCode::NOT_MODIFIED {
        return Some(0);
    }
    headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
}

/// Unified HTTP response with lazily consumed body.
///
/// # Examples
///
/// ```
/// use bytes::Bytes;
/// use http::HeaderMap;
/// use http::Method;
/// use http::StatusCode;
/// use qubit_http::HttpResponse;
/// use url::Url;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mut response = HttpResponse::new(
///     StatusCode::OK,
///     HeaderMap::new(),
///     Bytes::from_static(b"ready"),
///     Url::parse("https://example.com/status")?,
///     Method::GET,
/// );
/// assert_eq!(response.bytes().await?, Bytes::from_static(b"ready"));
/// # Ok(())
/// # }
/// ```
pub struct HttpResponse {
    /// Response metadata (status, headers, final URL, request method).
    pub(crate) meta: HttpResponseMeta,
    /// Backend, buffered, or already-taken response body.
    body: HttpResponseBodyState,
    /// Runtime state inherited from request/client.
    runtime: HttpResponseRuntime,
    /// Decode and error-preview options inherited from client options.
    options: HttpResponseOptions,
    /// Content length captured before response interceptors can edit headers.
    original_content_length: Option<u64>,
}

/// Adds missing response context to an SSE decoder error while preserving
/// specific fields, classification, and source already supplied by its origin.
///
/// # Parameters
/// - `error`: Decoder or body-read error to enrich.
/// - `method`: Request method used when producing the response.
/// - `url`: Request URL used when producing the response.
/// - `status`: HTTP response status.
/// - `redactor`: Policy used to render the error's diagnostic context.
///
/// # Returns
/// The error with missing method, URL, or status fields filled and the response
/// redaction policy attached.
fn attach_sse_error_context(
    mut error: HttpError,
    method: &Method,
    url: &Url,
    status: StatusCode,
    redactor: &Redactor,
) -> HttpError {
    if error.method.is_none() {
        error.method = Some(method.clone());
    }
    if error.url.is_none() {
        error.url = Some(url.clone());
    }
    if error.status.is_none() {
        error.status = Some(status);
    }
    error.with_log_redactor(redactor.clone())
}

impl fmt::Debug for HttpResponse {
    /// Formats response diagnostics while redacting sensitive metadata.
    ///
    /// # Parameters
    /// - `formatter`: Destination for the diagnostic representation.
    ///
    /// # Returns
    /// Formatting result from the destination.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut batch = self.options.log_redactor.diagnostic_batch();
        let url = batch.redact_http_url(self.meta.url().as_str());
        let request_url = batch.redact_http_url(self.runtime.request_url.as_str());
        let headers = batch.redact_http_headers(self.meta.headers());
        let output = batch.finish();
        let url = output.text(url);
        let request_url = output.text(request_url);
        let headers = output.text(headers);
        let body_state = match &self.body {
            HttpResponseBodyState::Backend(_) => "backend",
            HttpResponseBodyState::Buffered(_) => "buffered",
            HttpResponseBodyState::StreamingTaken => "streaming_taken",
        };
        let buffered_body_len = match &self.body {
            HttpResponseBodyState::Buffered(body) => Some(body.len()),
            _ => None,
        };
        formatter
            .debug_struct("HttpResponse")
            .field("status", &self.meta.status())
            .field("headers", &headers)
            .field("url", &url)
            .field("request_url", &request_url)
            .field("method", self.meta.method())
            .field("body_state", &body_state)
            .field("buffered_body_len", &buffered_body_len)
            .field("read_timeout", &self.runtime.read_timeout)
            .field("cancellation_token_present", &self.runtime.cancellation_token.is_some())
            .field("options", &self.options)
            .finish()
    }
}

impl HttpResponse {
    /// Validates that this response declares the Server-Sent Events media type.
    ///
    /// This optional check examines only the `Content-Type` header and does not
    /// read or consume the response body. Call it before decoding when the
    /// caller wants to enforce the SSE media type:
    ///
    /// ```no_run
    /// # async fn example(mut response: qubit_http::HttpResponse) -> qubit_http::HttpResult<()> {
    /// response.validate_sse_content_type()?;
    /// let mut events = response.sse_messages();
    /// # let _ = events;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Returns
    /// `Ok(())` when `Content-Type` is `text/event-stream` (parameters and
    /// casing are accepted).
    ///
    /// # Errors
    /// Returns [`HttpErrorKind::SseProtocol`] when the header is missing,
    /// invalid UTF-8, or does not declare the SSE media type.
    pub fn validate_sse_content_type(&self) -> HttpResult<()> {
        let method = self.meta().method().clone();
        let url = self.request_url().clone();
        let redactor = self.log_redactor();
        let Some(value) = self.headers().get(CONTENT_TYPE) else {
            return Err(HttpError::sse_protocol("Missing Content-Type header for SSE response")
                .with_status(self.status())
                .with_method(&method)
                .with_url(&url)
                .with_log_redactor(redactor.clone()));
        };
        let content_type = value.to_str().map_err(|_| {
            HttpError::sse_protocol("Invalid non-UTF8 Content-Type header for SSE response")
                .with_status(self.status())
                .with_method(&method)
                .with_url(&url)
                .with_log_redactor(redactor.clone())
        })?;
        if content_type::is_sse(content_type) {
            return Ok(());
        }
        Err(
            HttpError::sse_protocol("Expected Content-Type 'text/event-stream' for SSE response")
                .with_status(self.status())
                .with_method(&method)
                .with_url(&url)
                .with_log_redactor(redactor.clone()),
        )
    }

    /// Creates a buffered response from its status, headers, body, URL, and
    /// method.
    ///
    /// The body is retained in memory and returned by subsequent reads without
    /// contacting a backend stream.
    ///
    /// # Parameters
    /// - `status`: HTTP status returned by the server.
    /// - `headers`: Response headers associated with the body.
    /// - `body`: Fully buffered response bytes.
    /// - `url`: Final response URL.
    /// - `method`: Request method used to obtain the response.
    ///
    /// # Returns
    /// A response whose body is immediately available for reading.
    pub fn new(status: StatusCode, headers: HeaderMap, body: Bytes, url: Url, method: Method) -> Self {
        let original_content_length = parse_content_length_hint(&method, status, &headers);
        Self {
            meta: HttpResponseMeta::new(status, headers, url.clone(), method),
            body: HttpResponseBodyState::Buffered(body),
            runtime: HttpResponseRuntime::new(Duration::from_secs(30), None, url),
            options: HttpResponseOptions::default(),
            original_content_length,
        }
    }

    /// Creates a response that lazily reads the backend body with
    /// request-scoped options.
    ///
    /// # Parameters
    /// - `meta`: Response status, headers, final URL, and request method.
    /// - `backend`: Backend response whose body will be consumed on demand.
    /// - `read_timeout`: Maximum wait for each backend body read and complete
    ///   non-success error-body capture phase.
    /// - `cancellation_token`: Optional request cancellation signal.
    /// - `request_url`: Original URL used in read diagnostics.
    /// - `options`: Response decoding, size, and logging policy snapshot.
    /// # Returns
    /// A response backed by the unconsumed backend body.
    pub(crate) fn from_backend(
        meta: HttpResponseMeta,
        backend: reqwest::Response,
        read_timeout: Duration,
        cancellation_token: Option<HttpCancellationToken>,
        request_url: Url,
        options: HttpResponseOptions,
    ) -> Self {
        let original_content_length = parse_content_length_hint(meta.method(), meta.status(), meta.headers());
        Self {
            meta,
            body: HttpResponseBodyState::Backend(backend),
            runtime: HttpResponseRuntime::new(read_timeout, cancellation_token, request_url),
            options,
            original_content_length,
        }
    }

    /// Returns the response metadata shared with logging and interceptors.
    ///
    /// The returned reference is tied to this response and does not copy the
    /// headers or URL.
    #[inline]
    pub fn meta(&self) -> &HttpResponseMeta {
        &self.meta
    }

    /// Returns the HTTP status code received for this response.
    #[inline]
    pub fn status(&self) -> StatusCode {
        self.meta.status()
    }

    /// Restores the request-scoped token after the retry controller completes.
    ///
    /// Retry attempts let the controller own active-attempt cancellation so a
    /// request-level cancellation error cannot mask the structured retry
    /// terminal. A successful response keeps using the same token for body and
    /// SSE stream reads.
    ///
    /// # Parameters
    /// - `token`: Cancellation token used by retry and response reads.
    pub(crate) fn set_cancellation_token(&mut self, token: HttpCancellationToken) {
        self.runtime.cancellation_token = Some(token);
    }

    /// Returns the response headers without copying them.
    #[inline]
    pub fn headers(&self) -> &HeaderMap {
        self.meta.headers()
    }

    /// Returns the final URL after any redirects followed by the backend.
    #[inline]
    pub fn url(&self) -> &Url {
        self.meta.url()
    }

    /// Returns the original request URL used in body-read diagnostics.
    #[inline]
    pub fn request_url(&self) -> &Url {
        &self.runtime.request_url
    }

    /// Reports whether the response status belongs to the success class.
    #[must_use]
    #[inline]
    pub fn is_success(&self) -> bool {
        self.status().is_success()
    }

    /// Returns parsed `Retry-After` hint when status and headers provide one.
    #[inline]
    pub fn retry_after_hint(&self) -> Option<Duration> {
        self.meta.retry_after_hint()
    }

    /// Returns a previous body read failure, if any.
    ///
    /// # Returns
    /// `Some(HttpError)` when an earlier body read failed; otherwise `None`.
    fn previous_body_read_error(&self) -> Option<HttpError> {
        previous_body_read_error(&self.runtime.body_read_failure)
    }

    /// Creates the diagnostic error returned when a body stream was already
    /// taken.
    ///
    /// The request method, original request URL, response status, and response
    /// redaction policy are copied from this response so the error retains the
    /// same diagnostic context as other body-read failures.
    ///
    /// # Returns
    /// An error classified as [`HttpErrorKind::ResponseBodyAlreadyConsumed`]
    /// with this response's diagnostic context.
    fn response_body_already_consumed_error(&self) -> HttpError {
        HttpError::new(
            HttpErrorKind::ResponseBodyAlreadyConsumed,
            "Response body stream has already been consumed",
        )
        .with_method(self.meta.method())
        .with_url(self.request_url())
        .with_status(self.meta.status())
        .with_log_redactor(self.log_redactor().clone())
    }

    /// Stores the first body read failure for later read attempts.
    ///
    /// # Parameters
    /// - `error`: Error produced while reading the response body.
    fn remember_body_read_failure(&self, error: &HttpError) {
        remember_body_read_failure_state(&self.runtime.body_read_failure, error);
    }

    /// Returns `Ok(self)` for success statuses, otherwise maps a status error
    /// with `Retry-After` and response-body preview context.
    ///
    /// # Parameters
    /// - `message_prefix`: Context prepended to the status error message.
    ///
    /// # Returns
    /// This response when its status is successful.
    ///
    /// # Errors
    /// Returns a `Status` error for unsuccessful responses. Body capture
    /// failures and timeouts are retained in the capture diagnostics;
    /// cancellation returns `Cancelled`.
    pub(crate) async fn into_success_or_status_error(self, message_prefix: &str) -> HttpResult<Self> {
        let status = self.status();
        if status.is_success() {
            return Ok(self);
        }
        let retry_after = self.retry_after_hint();
        let method = self.meta.method().clone();
        let url = self.request_url().clone();
        let error_preview_limit = self.options.error_response_preview_limit;
        let error_body_headers = self.meta.headers().clone();
        let error_body_content_length = self.content_length_hint();
        let error_body_limit = self.options.error_response_body_limit;
        let log_redactor = self.log_redactor().clone();
        let capture = self
            .into_error_body_capture(error_preview_limit, error_body_limit)
            .await?;
        let redacted_url = log_redactor
            .redact_http_url(url.as_str())
            .into_text_or_marker("<redaction incomplete>");
        let message = format!(
            "{} with status {} for {} {}; response body preview: {}",
            message_prefix, status, method, redacted_url, capture.preview
        );
        let mut mapped = HttpError::from_status(status, message)
            .with_method(&method)
            .with_url(&url)
            .with_response_body_preview(capture.preview.clone())
            .with_log_redactor(log_redactor.clone());
        if let Some(retry_after) = retry_after {
            mapped = mapped.with_retry_after(retry_after);
        }
        mapped = mapped.with_status_response(crate::error::HttpStatusResponse::new(
            error_body_headers,
            capture.raw_body,
            capture.end,
            capture.error,
            error_body_content_length,
        ));
        Err(mapped)
    }

    /// Consumes this response and captures a bounded diagnostic preview and raw
    /// body prefix for a status error. Preview truncation is independent of raw
    /// capture completeness.
    ///
    /// # Returns
    /// A capture containing the rendered preview, raw body prefix, capture end
    /// reason, and an optional backend read error.
    ///
    /// # Errors
    /// Returns [`HttpErrorKind::Cancelled`](crate::HttpErrorKind::Cancelled)
    /// when the request cancellation token fires while preview bytes are being
    /// read.
    ///
    /// # Parameters
    /// - `preview_limit`: Maximum rendered preview size in bytes.
    /// - `raw_limit`: Maximum retained raw body size in bytes.
    ///
    /// # Returns
    /// Captured response body data and its terminal status.
    async fn into_error_body_capture(
        mut self,
        preview_limit: usize,
        raw_limit: usize,
    ) -> HttpResult<HttpErrorBodyCapture> {
        let limit = preview_limit.max(1);
        let state = std::mem::replace(&mut self.body, HttpResponseBodyState::StreamingTaken);
        let content_type = Self::content_type_header(self.meta.headers());
        let backend = match state {
            HttpResponseBodyState::Backend(backend) => backend,
            HttpResponseBodyState::Buffered(body) => {
                let raw_limit = raw_limit.max(1);
                let end = if body.len() > raw_limit {
                    crate::error::HttpBodyCaptureEnd::SizeLimit
                } else {
                    crate::error::HttpBodyCaptureEnd::Complete
                };
                let preview_truncated = body.len() > limit;
                let raw = body.slice(..body.len().min(raw_limit));
                let preview = Self::render_error_body_preview(
                    &body[..body.len().min(limit)],
                    Some(body.len()),
                    preview_truncated,
                    content_type.as_ref(),
                    &self.options.log_redactor,
                );
                return Ok(HttpErrorBodyCapture {
                    preview,
                    raw_body: raw,
                    end,
                    error: None,
                });
            }
            HttpResponseBodyState::StreamingTaken => {
                return Ok(HttpErrorBodyCapture {
                    preview: "<empty>".to_string(),
                    raw_body: Bytes::new(),
                    end: crate::error::HttpBodyCaptureEnd::BodyUnavailable,
                    error: None,
                });
            }
        };
        self.read_error_body_preview(backend, limit, raw_limit, content_type)
            .await
    }

    /// Returns full body bytes, consuming backend stream lazily on first call.
    ///
    /// # Returns
    /// The complete body bytes, or an error if reading fails or exceeds the
    /// configured limit. A successfully buffered body remains available to
    /// later calls.
    ///
    /// # Cancellation safety
    ///
    /// For an unbuffered response, dropping this future after it has taken the
    /// backend body and before buffering completes discards the backend and any
    /// partially read bytes. Later body readers return
    /// [`HttpErrorKind::ResponseBodyAlreadyConsumed`]. An outer
    /// `tokio::time::timeout` or a losing `tokio::select!` branch can cause
    /// this drop.
    ///
    /// Dropping a future before it is polled does not take the body. A
    /// successfully buffered body remains reusable. Cancellation and read
    /// timeouts returned by the built-in reader follow the existing
    /// retained-failure behavior instead. Configure the request's read timeout
    /// or cancellation token before sending when the reader should return a
    /// classified failure.
    /// Cancellation after buffering does not prevent reads of cached bytes.
    ///
    /// # Errors
    /// Returns the backend read, timeout, cancellation, or body-size error.
    pub async fn bytes(&mut self) -> HttpResult<Bytes> {
        let body_limit = self.options.response_body_size_limit;
        if let Some(error) = self.previous_body_read_error() {
            return Err(error);
        }
        let state = std::mem::replace(&mut self.body, HttpResponseBodyState::StreamingTaken);
        let mut backend = match state {
            HttpResponseBodyState::Buffered(body) => {
                if body.len() > body_limit {
                    let error = self.response_body_size_limit_error(body.len());
                    self.remember_body_read_failure(&error);
                    return Err(error);
                }
                self.body = HttpResponseBodyState::Buffered(body.clone());
                return Ok(body);
            }
            HttpResponseBodyState::Backend(backend) => backend,
            HttpResponseBodyState::StreamingTaken => {
                return Err(self.response_body_already_consumed_error());
            }
        };

        let method = self.meta.method().clone();
        let url = self.runtime.request_url.clone();
        let status = self.meta.status();
        let read_timeout = self.runtime.read_timeout;
        let cancellation_token = self.runtime.cancellation_token.clone();
        if let Some(content_length) = self.content_length_hint() {
            let exceeds_limit = match usize::try_from(content_length) {
                Ok(observed_size) => observed_size > body_limit,
                Err(_) => true,
            };
            if exceeds_limit {
                let observed_size = usize::try_from(content_length).unwrap_or(usize::MAX);
                let error = self.response_body_size_limit_error(observed_size);
                self.remember_body_read_failure(&error);
                return Err(error);
            }
        }
        let mut body = BytesMut::new();
        let mut body_budget = ResourceBudget::new("response body", body_limit);

        loop {
            let next = if let Some(token) = &cancellation_token {
                tokio::select! {
                    _ = token.cancelled() => {
                        let error = HttpError::cancelled("Request cancelled while reading response body")
                            .with_method(&method)
                            .with_url(&url)
                            .with_status(status)
                            .with_log_redactor(
                                self.log_redactor().clone(),
                            );
                        self.remember_body_read_failure(&error);
                        return Err(error);
                    }
                    item = tokio::time::timeout(read_timeout, backend.chunk()) => item,
                }
            } else {
                tokio::time::timeout(read_timeout, backend.chunk()).await
            };

            match next {
                Ok(Ok(Some(chunk))) => {
                    if let Err(error) = body_budget.try_consume(chunk.len()) {
                        let observed_size = error.used().saturating_add(error.requested());
                        let error = self.response_body_size_limit_error(observed_size);
                        self.remember_body_read_failure(&error);
                        return Err(error);
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(Ok(None)) => {
                    let body = body.freeze();
                    self.body = HttpResponseBodyState::Buffered(body.clone());
                    return Ok(body);
                }
                Ok(Err(error)) => {
                    let error = map_response_read_error(error, method, url, status, self.log_redactor());
                    self.remember_body_read_failure(&error);
                    return Err(error);
                }
                Err(_) => {
                    let error = HttpError::read_timeout(format!(
                        "Read timeout after {:?} while reading response body",
                        read_timeout
                    ))
                    .with_method(self.meta.method())
                    .with_url(&self.runtime.request_url)
                    .with_status(status)
                    .with_log_redactor(self.log_redactor().clone());
                    self.remember_body_read_failure(&error);
                    return Err(error);
                }
            }
        }
    }

    /// Returns body as stream; if already buffered, returns a stream backed by
    /// cached bytes without consuming the cache. Repeated streams and byte
    /// reads therefore return the same buffered body.
    ///
    /// The stream takes ownership of an unbuffered backend body. It can only be
    /// obtained once; a buffered body can be streamed from its cached bytes.
    ///
    /// # Returns
    /// A stream of body chunks. A successfully buffered body remains readable
    /// after request cancellation; cancellation stops reads that still require
    /// backend I/O.
    ///
    /// # Errors
    /// Returns an error if the body was already consumed, an earlier read
    /// failed, or an unbuffered response was cancelled before streaming starts.
    pub fn stream(&mut self) -> HttpResult<HttpByteStream> {
        if let Some(error) = self.previous_body_read_error() {
            return Err(error);
        }
        if let HttpResponseBodyState::Buffered(body) = &self.body {
            let body = body.clone();
            return Ok(Box::pin(futures_stream::once(async move { Ok(body) })));
        }
        if let Some(error) = self.cancelled_error_if_needed("Streaming response cancelled before reading response body")
        {
            return Err(error);
        }
        let state = std::mem::replace(&mut self.body, HttpResponseBodyState::StreamingTaken);
        let backend = match state {
            HttpResponseBodyState::Buffered(_) => unreachable!("buffered body returned above"),
            HttpResponseBodyState::Backend(backend) => backend,
            HttpResponseBodyState::StreamingTaken => {
                return Err(self.response_body_already_consumed_error());
            }
        };

        let method = self.meta.method().clone();
        let url = self.runtime.request_url.clone();
        let status = self.meta.status();
        let read_timeout = self.runtime.read_timeout;
        let cancellation_token = self.runtime.cancellation_token.clone();
        let body_read_failure = self.runtime.body_read_failure.clone();
        let log_redactor = self.log_redactor().clone();
        let mut stream = backend.bytes_stream();
        let wrapped = stream! {
            loop {
                let next = if let Some(token) = &cancellation_token {
                    tokio::select! {
                        _ = token.cancelled() => {
                            let error = HttpError::cancelled("Streaming response cancelled while reading body")
                                .with_method(&method)
                                .with_url(&url)
                                .with_status(status)
                                .with_log_redactor(log_redactor.clone());
                            remember_body_read_failure_state(&body_read_failure, &error);
                            yield Err(error);
                            break;
                        }
                        item = tokio::time::timeout(read_timeout, stream.next()) => item,
                    }
                } else {
                    tokio::time::timeout(read_timeout, stream.next()).await
                };
                match next {
                    Ok(Some(Ok(bytes))) => yield Ok(bytes),
                    Ok(Some(Err(error))) => {
                        let mapped = map_response_read_error(
                            error,
                            method.clone(),
                            url.clone(),
                            status,
                            &log_redactor,
                        );
                        remember_body_read_failure_state(&body_read_failure, &mapped);
                        yield Err(mapped);
                        break;
                    }
                    Ok(None) => break,
                    Err(_) => {
                        let error = HttpError::read_timeout(format!(
                            "Read timeout after {:?} while streaming response",
                            read_timeout
                        ))
                        .with_method(&method)
                        .with_url(&url)
                        .with_status(status)
                        .with_log_redactor(log_redactor.clone());
                        remember_body_read_failure_state(&body_read_failure, &error);
                        yield Err(error);
                        break;
                    }
                }
            }
        };
        Ok(Box::pin(wrapped))
    }

    /// Interprets the complete response body as UTF-8 text.
    ///
    /// # Returns
    /// Decoded text when the body contains valid UTF-8.
    ///
    /// # Errors
    /// Returns a body read error or a decoding error for invalid UTF-8.
    ///
    /// # Cancellation safety
    ///
    /// This method aggregates through [`Self::bytes`] and has the same
    /// cancellation-safety boundary: dropping a polled read future before
    /// buffering completes can consume an unbuffered backend body permanently.
    pub async fn text(&mut self) -> HttpResult<String> {
        let body = self.bytes().await?;
        String::from_utf8(body.to_vec()).map_err(|error| {
            HttpError::decode(format!("Failed to decode response body as UTF-8: {}", error))
                .with_status(self.meta.status())
                .with_method(self.meta.method())
                .with_url(self.request_url())
                .with_log_redactor(self.log_redactor().clone())
        })
    }

    /// Deserializes the complete response body as JSON into `T`.
    ///
    /// # Type Parameters
    /// - `T`: Owned type to deserialize from the response JSON.
    ///
    /// # Returns
    /// The decoded value when the body is valid JSON matching `T`.
    ///
    /// # Errors
    /// Returns a body read error or a JSON decoding error.
    ///
    /// # Cancellation safety
    ///
    /// This method aggregates through [`Self::bytes`] and has the same
    /// cancellation-safety boundary: dropping a polled read future before
    /// buffering completes can consume an unbuffered backend body permanently.
    pub async fn json<T>(&mut self) -> HttpResult<T>
    where
        T: DeserializeOwned,
    {
        let body = self.bytes().await?;
        JsonDecoder::with_limits(json_decode_limits(
            self.options.response_body_size_limit,
            self.options.json_value_limits,
        ))
        .decode_utf8(&body)
        .map_err(|error| {
            HttpError::decode("Failed to decode response JSON")
                .with_status(self.meta.status())
                .with_method(self.meta.method())
                .with_url(self.request_url())
                .with_source(error)
                .with_log_redactor(self.log_redactor().clone())
        })
    }

    /// Overrides structural and decoded-payload limits used by JSON response
    /// and SSE decoding on this response.
    ///
    /// # Parameters
    /// - `limits`: JSON value limits replacing the client defaults.
    ///
    /// # Returns
    /// This response with the limits applied for chained configuration.
    pub fn json_value_limits(mut self, limits: JsonValueLimits) -> Self {
        self.options.json_value_limits = limits;
        self
    }

    /// Overrides the maximum allowed size (in bytes) for one SSE line on this
    /// response.
    ///
    /// Values below 1 are clamped to 1. Returns `self` so callers can chain
    /// configuration before consuming the body with [`Self::sse_messages`]
    /// or [`Self::sse_chunks`] (together with [`Self::sse_json_mode`],
    /// [`Self::sse_done_marker_policy`], etc.).
    ///
    /// # Parameters
    /// - `max_line_bytes`: Requested maximum line size, clamped to at least one
    ///   byte.
    ///
    /// # Returns
    /// This response with the line limit applied.
    pub fn sse_max_line_bytes(mut self, max_line_bytes: usize) -> Self {
        self.options.sse_max_line_bytes = max_line_bytes.max(1);
        self
    }

    /// Overrides the maximum allowed size (in bytes) for one SSE frame on this
    /// response.
    ///
    /// The limit counts cumulative UTF-8 bytes of non-empty, non-comment lines
    /// between blank-line boundaries. Field names and values, including
    /// unknown fields, count; line terminators and comment lines do not.
    /// Comment lines remain subject to the line limit. This is not a wire-size
    /// or whole-stream limit.
    ///
    /// Values below 1 are clamped to 1. Returns `self` for chained
    /// configuration.
    ///
    /// # Parameters
    /// - `max_frame_bytes`: Requested maximum frame size, clamped to at least
    ///   one byte.
    ///
    /// # Returns
    /// This response with the frame limit applied.
    pub fn sse_max_frame_bytes(mut self, max_frame_bytes: usize) -> Self {
        self.options.sse_max_frame_bytes = max_frame_bytes.max(1);
        self
    }

    /// Overrides the JSON decoding mode used by [`Self::sse_chunks`] on this
    /// response.
    ///
    /// # Parameters
    /// - `mode`: JSON decoding mode applied to SSE data payloads.
    ///
    /// # Returns
    /// This response with the selected mode for chained configuration.
    #[inline]
    pub fn sse_json_mode(mut self, mode: SseJsonMode) -> Self {
        self.options.sse_json_mode = mode;
        self
    }

    /// Overrides how [`Self::sse_chunks`] detects end-of-stream from trimmed
    /// `data:` payloads.
    ///
    /// # Parameters
    /// - `policy`: Marker policy used to recognize terminal SSE payloads.
    ///
    /// # Returns
    /// This response with the selected policy for chained configuration.
    pub fn sse_done_marker_policy(mut self, policy: DoneMarkerPolicy) -> Self {
        self.options.sse_done_marker_policy = policy;
        self
    }

    /// Sets whether JSON SSE decoding requires a done marker before EOF.
    ///
    /// `RequireDoneMarker` yields an `SseProtocol` error when the stream ends
    /// without the configured marker. If the marker policy is disabled or
    /// empty, the first stream item reports the invalid combination.
    ///
    /// # Parameters
    /// - `policy`: Completion requirement applied to JSON SSE decoding.
    ///
    /// # Returns
    /// This response with the selected completion policy.
    pub fn sse_completion_policy(mut self, policy: SseCompletionPolicy) -> Self {
        self.options.sse_completion_policy = policy;
        self
    }

    /// Decodes body stream as SSE messages using this response's SSE line/frame
    /// byte limits (from client defaults unless overridden via
    /// [`Self::sse_max_line_bytes`] / [`Self::sse_max_frame_bytes`]).
    ///
    /// This is a body decoder and does not validate the response's
    /// `Content-Type`. Check for `text/event-stream` before calling it when the
    /// upstream media type is part of your protocol contract.
    /// A successfully buffered body remains decodable after cancellation
    /// because it no longer requires backend I/O.
    ///
    /// # Returns
    /// A stream of decoded SSE messages or decoding errors.
    pub fn sse_messages(mut self) -> SseMessageStream {
        let max_line_bytes = self.options.sse_max_line_bytes;
        let max_frame_bytes = self.options.sse_max_frame_bytes;
        let log_redactor = self.log_redactor().clone();
        let method = self.meta.method().clone();
        let url = self.request_url().clone();
        let status = self.status();
        let decoded: SseMessageStream = match self.stream() {
            Ok(stream) => crate::sse::decode_messages_from_stream_with_limits(stream, max_line_bytes, max_frame_bytes),
            Err(error) => Box::pin(futures_stream::once(async move { Err(error) })),
        };
        Box::pin(decoded.map(move |result| {
            result.map_err(|error| attach_sse_error_context(error, &method, &url, status, &log_redactor))
        }))
    }

    /// Decodes body stream as internal SSE records for reconnect state
    /// handling.
    ///
    /// # Returns
    /// Stream of internal records, or one error item when the body cannot be
    /// opened.
    pub(crate) fn sse_records(mut self) -> crate::sse::SseRecordStream {
        let max_line_bytes = self.options.sse_max_line_bytes;
        let max_frame_bytes = self.options.sse_max_frame_bytes;
        let log_redactor = self.log_redactor().clone();
        let method = self.meta.method().clone();
        let url = self.request_url().clone();
        let status = self.status();
        let decoded: crate::sse::SseRecordStream = match self.stream() {
            Ok(stream) => crate::sse::decode_records_from_stream_with_limits(stream, max_line_bytes, max_frame_bytes),
            Err(error) => Box::pin(futures_stream::once(async move { Err(error) })),
        };
        Box::pin(decoded.map(move |result| {
            result.map_err(|error| attach_sse_error_context(error, &method, &url, status, &log_redactor))
        }))
    }

    /// Decodes SSE `data:` lines as JSON chunks using this response's SSE JSON
    /// mode, done-marker policy, and line/frame limits (see
    /// [`Self::sse_json_mode`], [`Self::sse_done_marker_policy`],
    /// [`Self::sse_max_line_bytes`], [`Self::sse_max_frame_bytes`]).
    ///
    /// This is a body decoder and does not validate `Content-Type`.
    /// [`crate::HttpClient::execute_sse_with_reconnect`] validates
    /// `text/event-stream`; direct callers must check the header themselves
    /// when their protocol requires it.
    ///
    /// # Type Parameters
    /// - `T`: Owned type decoded from each SSE data payload.
    ///
    /// # Returns
    /// A stream of decoded chunks and any configured done marker. With
    /// [`SseJsonMode::Lenient`], invalid JSON payloads are skipped; with
    /// [`SseJsonMode::Strict`], the first JSON decode error is emitted and the
    /// stream then terminates. Stream, protocol, and resource-limit errors may
    /// also terminate the stream. When
    /// [`SseCompletionPolicy::RequireDoneMarker`] is configured, reaching EOF
    /// without the required marker emits an SSE protocol error.
    pub fn sse_chunks<T>(mut self) -> SseChunkStream<T>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let done_policy = self.options.sse_done_marker_policy.clone();
        let mode = self.options.sse_json_mode;
        let max_line_bytes = self.options.sse_max_line_bytes;
        let max_frame_bytes = self.options.sse_max_frame_bytes;
        let json_value_limits = self.options.json_value_limits;
        let completion_policy = self.options.sse_completion_policy;
        let log_redactor = self.log_redactor().clone();
        let method = self.meta.method().clone();
        let url = self.request_url().clone();
        let status = self.status();
        let decoded: SseChunkStream<T> = match self.stream() {
            Ok(stream) => crate::sse::decode_json_chunks_from_stream_with_limits(
                stream,
                done_policy,
                mode,
                max_line_bytes,
                max_frame_bytes,
                json_value_limits,
                completion_policy,
            ),
            Err(error) => Box::pin(futures_stream::once(async move { Err(error) })),
        };
        Box::pin(decoded.map(move |result| {
            result.map_err(|error| attach_sse_error_context(error, &method, &url, status, &log_redactor))
        }))
    }

    /// Returns the shared log redactor used for response diagnostics and
    /// errors.
    #[inline]
    pub(crate) fn log_redactor(&self) -> &Redactor {
        &self.options.log_redactor
    }

    /// Returns a buffered body reference for response logging if available.
    ///
    /// # Returns
    /// `Some(&Bytes)` when response body has already been buffered.
    #[inline]
    pub(crate) fn buffered_body_for_logging(&self) -> Option<&Bytes> {
        match &self.body {
            HttpResponseBodyState::Buffered(body) => Some(body),
            _ => None,
        }
    }

    /// Reads bounded preview and raw capture bytes from a response body for
    /// status errors, tracking their truncation independently. The read
    /// timeout is a cumulative budget starting when this capture begins; if it
    /// expires, the caller retains the status and any partial raw body marked
    /// incomplete.
    ///
    /// # Errors
    /// Returns [`HttpErrorKind::Cancelled`](crate::HttpErrorKind::Cancelled)
    /// when the supplied cancellation token fires while waiting for preview
    /// bytes.
    ///
    /// # Parameters
    /// - `response`: Backend response whose error body is read.
    /// - `max_bytes`: Maximum rendered preview size in bytes.
    /// - `raw_limit`: Maximum retained raw body size in bytes.
    /// - `content_type`: Optional response content type used to render preview.
    ///
    /// # Returns
    /// Captured response body data and its terminal status.
    async fn read_error_body_preview(
        &self,
        mut response: reqwest::Response,
        max_bytes: usize,
        raw_limit: usize,
        content_type: Option<HeaderValue>,
    ) -> HttpResult<HttpErrorBodyCapture> {
        let limit = max_bytes.max(1);
        let read_timeout = self.runtime.read_timeout;
        let cancellation_token = self.runtime.cancellation_token.clone();
        let method = self.meta.method().clone();
        let url = self.runtime.request_url.clone();
        let status = self.meta.status();
        let source_len = self
            .content_length_hint()
            .and_then(|length| usize::try_from(length).ok());
        let mut preview = Vec::new();
        let mut raw = Vec::new();
        let mut preview_truncated = false;
        let mut raw_truncated = false;
        let mut end = crate::error::HttpBodyCaptureEnd::Complete;
        let capture_limit = limit.max(raw_limit.max(1)).saturating_add(1);
        let mut capture_budget = ResourceBudget::new("status error response body preview", capture_limit);
        let capture_started_at = Instant::now();

        loop {
            if cancellation_token
                .as_ref()
                .is_some_and(HttpCancellationToken::is_cancelled)
            {
                return Err(
                    HttpError::cancelled("Request cancelled while reading status error response body preview")
                        .with_method(&method)
                        .with_url(&url)
                        .with_status(status)
                        .with_log_redactor(self.log_redactor().clone()),
                );
            }
            let remaining = read_timeout.saturating_sub(capture_started_at.elapsed());
            if remaining.is_zero() {
                return Ok(HttpErrorBodyCapture {
                    preview: format!("<error body unavailable: read timeout after {:?}>", read_timeout),
                    raw_body: Bytes::from(raw),
                    end: crate::error::HttpBodyCaptureEnd::ReadTimeout,
                    error: None,
                });
            }
            let next = if let Some(token) = cancellation_token.as_ref() {
                tokio::select! {
                    _ = token.cancelled() => {
                        return Err(HttpError::cancelled(
                            "Request cancelled while reading status error response body preview",
                        )
                        .with_method(&method)
                        .with_url(&url)
                        .with_status(status)
                        .with_log_redactor(
                            self.log_redactor().clone(),
                        ));
                    }
                    item = tokio::time::timeout(remaining, response.chunk()) => item,
                }
            } else {
                tokio::time::timeout(remaining, response.chunk()).await
            };
            match next {
                Ok(Ok(Some(chunk))) => {
                    let captured = capture_budget.consume_available(chunk.len());
                    let preview_remaining = limit.saturating_sub(preview.len());
                    if preview_remaining < captured {
                        preview_truncated = true;
                    }
                    preview.extend_from_slice(&chunk[..captured.min(preview_remaining)]);
                    let raw_remaining = raw_limit.saturating_sub(raw.len());
                    if raw_remaining < chunk.len() {
                        raw_truncated = true;
                    }
                    raw.extend_from_slice(&chunk[..chunk.len().min(raw_remaining)]);
                    if captured < chunk.len() {
                        preview_truncated |= preview_remaining < chunk.len();
                        raw_truncated |= raw_remaining < chunk.len();
                        break;
                    }
                    if capture_budget.used() == capture_limit && (preview_truncated || raw_truncated) {
                        break;
                    }
                }
                Ok(Ok(None)) => break,
                Ok(Err(error)) => {
                    let error = error.without_url();
                    let preview = "<error body unavailable: failed to read response body>".to_string();
                    let capture_error: Arc<dyn Error + Send + Sync + 'static> = Arc::new(error);
                    end = crate::error::HttpBodyCaptureEnd::ReadFailure;
                    return Ok(HttpErrorBodyCapture {
                        preview,
                        raw_body: Bytes::from(raw),
                        end,
                        error: Some(capture_error),
                    });
                }
                Err(_) => {
                    return Ok(HttpErrorBodyCapture {
                        preview: format!("<error body unavailable: read timeout after {:?}>", read_timeout),
                        raw_body: Bytes::from(raw),
                        end: crate::error::HttpBodyCaptureEnd::ReadTimeout,
                        error: None,
                    });
                }
            }
        }
        if preview.len() > limit {
            preview.truncate(limit);
            preview_truncated = true;
        }
        preview_truncated |= source_len.is_some_and(|length| length > limit);
        raw_truncated |= source_len.is_some_and(|length| length > raw_limit);
        if raw_truncated {
            end = crate::error::HttpBodyCaptureEnd::SizeLimit;
        }
        Ok(HttpErrorBodyCapture {
            preview: Self::render_error_body_preview(
                &preview,
                source_len,
                preview_truncated,
                content_type.as_ref(),
                &self.options.log_redactor,
            ),
            raw_body: Bytes::from(raw),
            end,
            error: None,
        })
    }

    /// Returns a cancellation error with response read context when cancelled.
    ///
    /// # Parameters
    /// - `message`: Cancellation message to include in the error.
    ///
    /// # Returns
    /// `Some(HttpError)` when this response has a cancelled token; otherwise
    /// `None`.
    fn cancelled_error_if_needed(&self, message: &str) -> Option<HttpError> {
        if self
            .runtime
            .cancellation_token
            .as_ref()
            .is_some_and(HttpCancellationToken::is_cancelled)
        {
            Some(
                HttpError::cancelled(message.to_string())
                    .with_method(self.meta.method())
                    .with_url(&self.runtime.request_url)
                    .with_status(self.meta.status())
                    .with_log_redactor(self.log_redactor().clone()),
            )
        } else {
            None
        }
    }

    /// Returns `Content-Length` parsed from response headers when present and
    /// valid.
    fn content_length_hint(&self) -> Option<u64> {
        self.original_content_length
    }

    /// Returns a response-body aggregation limit error with request context.
    ///
    /// # Parameters
    /// - `observed_size`: Number of body bytes observed before exceeding limit.
    ///
    /// # Returns
    /// An error with the response limit and request context attached.
    fn response_body_size_limit_error(&self, observed_size: usize) -> HttpError {
        let limit = self.options.response_body_size_limit;
        HttpError::new(
            HttpErrorKind::ResponseBodyTooLarge,
            format!("Response body exceeds configured limit of {limit} bytes (observed {observed_size} bytes)"),
        )
        .with_method(self.meta.method())
        .with_url(&self.runtime.request_url)
        .with_status(self.meta.status())
        .with_log_redactor(self.log_redactor().clone())
    }

    /// Renders captured status-error bytes through the response redactor.
    ///
    /// # Parameters
    ///
    /// * `bytes` - Captured body prefix.
    /// * `source_len` - Exact complete source length when known.
    /// * `truncated` - Whether source bytes were omitted from the capture.
    /// * `content_type` - Optional native Content-Type used for parser choice.
    /// * `log_redactor` - Response-scoped redactor and policy snapshot.
    ///
    /// # Returns
    ///
    /// `<empty>` for a complete empty body, otherwise bounded log-safe text.
    fn render_error_body_preview(
        bytes: &[u8],
        source_len: Option<usize>,
        truncated: bool,
        content_type: Option<&HeaderValue>,
        log_redactor: &Redactor,
    ) -> String {
        if bytes.is_empty() && !truncated {
            return "<empty>".to_owned();
        }
        let capture = if truncated {
            source_len.map_or_else(
                || BodyCapture::truncated_unknown(bytes),
                |total_len| {
                    BodyCapture::truncated(bytes, total_len).unwrap_or_else(|_| BodyCapture::truncated_unknown(bytes))
                },
            )
        } else {
            BodyCapture::complete(bytes)
        };
        let body = log_redactor.redact_http_body(capture, content_type);
        body.into_text_or_marker("<redaction incomplete>").into_string()
    }

    /// Extracts a Content-Type header value.
    ///
    /// # Parameters
    /// - `headers`: Headers to inspect.
    ///
    /// # Returns
    /// Owned Content-Type value when present, including non-UTF-8 values.
    fn content_type_header(headers: &HeaderMap) -> Option<HeaderValue> {
        headers.get(CONTENT_TYPE).cloned()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http::Method;
    use http::StatusCode;
    use qubit_redact::Redactor;
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    use super::HttpResponse;
    use super::HttpResponseMeta;
    use super::HttpResponseOptions;
    use super::attach_sse_error_context;
    use crate::HttpError;
    use crate::HttpErrorKind;
    use crate::response::internal::HttpResponseBodyState;

    #[tokio::test]
    async fn test_decode_errors_use_original_request_url() {
        let original = url::Url::parse("https://example.com/original").unwrap();
        let displayed = url::Url::parse("https://example.com/rewritten").unwrap();
        for is_json in [false, true] {
            let body = if is_json {
                bytes::Bytes::from_static(b"not-json")
            } else {
                bytes::Bytes::from_static(b"\xff")
            };
            let mut response = HttpResponse::new(
                StatusCode::OK,
                http::HeaderMap::new(),
                body,
                original.clone(),
                Method::POST,
            );
            response.meta.set_url(displayed.clone());
            let error = if is_json {
                response.json::<serde_json::Value>().await.unwrap_err()
            } else {
                response.text().await.unwrap_err()
            };
            assert_eq!(error.kind, HttpErrorKind::Decode);
            assert_eq!(error.method, Some(Method::POST));
            assert_eq!(error.status, Some(StatusCode::OK));
            assert_eq!(error.url.as_ref(), Some(&original));
            assert_eq!(response.url(), &displayed);
            assert_eq!(response.request_url(), &original);
            if is_json {
                assert!(std::error::Error::source(&error).is_some());
            }
        }
    }

    #[test]
    fn test_attach_sse_error_context_preserves_existing_error_fields() {
        let original_url = url::Url::parse("https://original.example/path").unwrap();
        let supplied_url = url::Url::parse("https://supplied.example/other").unwrap();
        let original_method = Method::POST;
        let error = HttpError::transport("existing transport error")
            .with_method(&original_method)
            .with_url(&original_url)
            .with_status(StatusCode::BAD_GATEWAY)
            .with_source(std::io::Error::other("source sentinel"));

        let enriched = attach_sse_error_context(
            error,
            &Method::GET,
            &supplied_url,
            StatusCode::OK,
            &Redactor::application_default(),
        );

        assert_eq!(enriched.kind, HttpErrorKind::Transport);
        assert_eq!(enriched.method, Some(original_method));
        assert_eq!(enriched.url, Some(original_url));
        assert_eq!(enriched.status, Some(StatusCode::BAD_GATEWAY));
        assert_eq!(
            enriched
                .source
                .as_deref()
                .and_then(|source| source.downcast_ref::<std::io::Error>())
                .map(std::io::Error::to_string)
                .as_deref(),
            Some("source sentinel")
        );
    }

    async fn make_backend(method: Method, status: u16) -> (reqwest::Response, url::Url) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
            }
            stream
                .write_all(
                    format!("HTTP/1.1 {status} response\r\nContent-Length: 100\r\nConnection: close\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();
        });
        let url = url::Url::parse(&format!("http://{address}/")).unwrap();
        let backend = reqwest::Client::new()
            .request(method, url.clone())
            .send()
            .await
            .unwrap();
        (backend, url)
    }

    #[tokio::test]
    async fn test_status_capture_stops_when_truncation_is_known() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (sent_tx, sent_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
            }
            stream.write_all(b"HTTP/1.1 400 Bad Request\r\nTransfer-Encoding: chunked\r\nContent-Type: text/plain\r\n\r\n5\r\nabcde\r\n").await.unwrap();
            stream.flush().await.unwrap();
            let _ = sent_tx.send(());
            let _ = release_rx.await;
        });
        let url = url::Url::parse(&format!("http://{address}/")).unwrap();
        let backend = reqwest::Client::new().get(url.clone()).send().await.unwrap();
        sent_rx.await.unwrap();
        let meta = HttpResponseMeta::new(
            backend.status(),
            backend.headers().clone(),
            backend.url().clone(),
            Method::GET,
        );
        let options = HttpResponseOptions {
            error_response_preview_limit: 4,
            error_response_body_limit: 4,
            ..HttpResponseOptions::default()
        };
        let response = HttpResponse::from_backend(meta, backend, Duration::from_secs(30), None, url, options);
        let error = tokio::time::timeout(Duration::from_secs(1), response.into_success_or_status_error("failed"))
            .await
            .expect("known truncation should finish before EOF")
            .unwrap_err();
        let captured = error.status_response().unwrap();
        assert_eq!(captured.body().as_ref(), b"abcd");
        assert!(captured.is_truncated());
        let _ = release_tx.send(());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_status_capture_reports_unavailable_taken_body() {
        let (backend, url) = make_backend(Method::GET, 404).await;
        let meta = HttpResponseMeta::new(
            backend.status(),
            backend.headers().clone(),
            backend.url().clone(),
            Method::GET,
        );
        let mut response = HttpResponse::from_backend(
            meta,
            backend,
            Duration::from_secs(1),
            None,
            url,
            HttpResponseOptions::default(),
        );
        response.body = HttpResponseBodyState::StreamingTaken;
        let error = response.into_success_or_status_error("failed").await.unwrap_err();
        let capture = error.status_response().unwrap();
        assert_eq!(capture.capture_end(), crate::HttpBodyCaptureEnd::BodyUnavailable);
        assert!(capture.is_truncated());
        assert!(capture.capture_error().is_none());
    }
}
