// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unified HTTP response type and helpers.

use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use async_stream::stream;
use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream as futures_stream;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use http::StatusCode;
use http::header::CONTENT_LENGTH;
use http::header::CONTENT_TYPE;
use qubit_budget::ResourceBudget;
use qubit_json::JsonDecodeOptions;
use qubit_json::LenientJsonDecoder;
use qubit_redact::http::HttpRedactor;
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::HttpResponseMeta;
use super::HttpResponseOptions;
use crate::HttpByteStream;
use crate::HttpError;
use crate::HttpErrorKind;
use crate::HttpResult;
use crate::content_type;
use crate::error::ReqwestErrorPhase;
use crate::error::backend_error_mapper::map_reqwest_error;
use crate::redact::RedactedDebugger;
use crate::sse::DoneMarkerPolicy;
use crate::sse::SseChunkStream;
use crate::sse::SseJsonMode;
use crate::sse::SseMessageStream;

/// Snapshot of a body read failure retained after the backend body is consumed.
#[derive(Debug, Clone)]
struct BodyReadFailure {
    /// Error category from the original failure.
    kind: HttpErrorKind,
    /// Original failure message.
    message: String,
    /// Optional request method context.
    method: Option<Method>,
    /// Optional request URL context.
    url: Option<Url>,
    /// Optional HTTP status context.
    status: Option<StatusCode>,
    /// Exact policy snapshot used by the originating response.
    log_redactor: HttpRedactor,
}

impl BodyReadFailure {
    /// Captures cloneable diagnostic fields from a read error.
    ///
    /// # Parameters
    /// - `error`: Original body read failure.
    ///
    /// # Returns
    /// Cloneable failure snapshot for subsequent reads.
    fn from_error(error: &HttpError) -> Self {
        Self {
            kind: error.kind,
            message: error.message.clone(),
            method: error.method.clone(),
            url: error.url.clone(),
            status: error.status,
            log_redactor: error.log_redactor.clone(),
        }
    }

    /// Rebuilds an [`HttpError`] for a later read attempt.
    ///
    /// # Returns
    /// Error preserving the original kind and cloneable context.
    fn to_error(&self) -> HttpError {
        let mut error = HttpError::new(
            self.kind,
            format!("previous response body read failed: {}", self.message),
        );
        if let Some(method) = self.method.as_ref() {
            error = error.with_method(method);
        }
        if let Some(url) = self.url.as_ref() {
            error = error.with_url(url);
        }
        if let Some(status) = self.status {
            error = error.with_status(status);
        }
        error.with_log_redactor(self.log_redactor.clone())
    }
}

/// Shared response body failure state.
type BodyReadFailureState = Arc<Mutex<Option<BodyReadFailure>>>;

/// Maps one backend response body read error into this crate's error model.
///
/// # Parameters
/// - `error`: Backend error returned while reading response body bytes.
/// - `method`: Request method used for diagnostics.
/// - `url`: Request URL used for diagnostics.
/// - `status`: Response status attached to the resulting diagnostic error.
/// - `log_redaction_policy`: Policy snapshot used to render error context.
///
/// # Returns
/// [`HttpErrorKind::ReadTimeout`] for timeout errors, otherwise
/// [`HttpErrorKind::Transport`] for backend body read failures.
fn map_response_read_error(
    error: reqwest::Error,
    method: Method,
    url: Url,
    status: StatusCode,
    log_redactor: &HttpRedactor,
) -> HttpError {
    if error.is_timeout() {
        return map_reqwest_error(
            error,
            HttpErrorKind::Transport,
            ReqwestErrorPhase::Read,
            method,
            url,
        )
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

/// Runtime state bound to one response instance.
#[derive(Debug, Clone)]
struct HttpResponseRuntime {
    /// Per-response read timeout inherited from request/client.
    read_timeout: Duration,
    /// Optional cancellation token inherited from request.
    cancellation_token: Option<CancellationToken>,
    /// Request URL used in read/cancellation error context.
    request_url: Url,
    /// First response body read failure, if the backend stream failed after
    /// being taken from this response.
    body_read_failure: BodyReadFailureState,
}

impl HttpResponseRuntime {
    fn new(
        read_timeout: Duration,
        cancellation_token: Option<CancellationToken>,
        request_url: Url,
    ) -> Self {
        Self {
            read_timeout,
            cancellation_token,
            request_url,
            body_read_failure: Arc::new(Mutex::new(None)),
        }
    }
}

/// Unified HTTP response with lazily consumed body.
pub struct HttpResponse {
    /// Response metadata (status, headers, final URL, request method).
    pub(crate) meta: HttpResponseMeta,
    /// Raw backend response until consumed.
    backend: Option<reqwest::Response>,
    /// Cached full body bytes after eager or lazy read.
    buffered_body: Option<Bytes>,
    /// Runtime state inherited from request/client.
    runtime: HttpResponseRuntime,
    /// Decode and error-preview options inherited from client options.
    options: HttpResponseOptions,
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let debugger = RedactedDebugger::new(&self.options.log_redactor);
        let session = debugger.session();
        let url = debugger.url_with_session(self.meta.url(), &session);
        let request_url =
            debugger.url_with_session(&self.runtime.request_url, &session);
        formatter
            .debug_struct("HttpResponse")
            .field("status", &self.meta.status())
            .field(
                "headers",
                &debugger.headers_with_session(self.meta.headers(), &session),
            )
            .field("url", &url)
            .field("request_url", &request_url)
            .field("method", self.meta.method())
            .field("backend_present", &self.backend.is_some())
            .field(
                "buffered_body_len",
                &self.buffered_body.as_ref().map(Bytes::len),
            )
            .field("read_timeout", &self.runtime.read_timeout)
            .field(
                "cancellation_token_present",
                &self.runtime.cancellation_token.is_some(),
            )
            .field("options", &self.options)
            .finish()
    }
}

impl HttpResponse {
    /// Creates a buffered response.
    pub fn new(
        status: StatusCode,
        headers: HeaderMap,
        body: Bytes,
        url: Url,
        method: Method,
    ) -> Self {
        Self {
            meta: HttpResponseMeta::new(status, headers, url.clone(), method),
            backend: None,
            buffered_body: Some(body),
            runtime: HttpResponseRuntime::new(
                Duration::from_secs(30),
                None,
                url,
            ),
            options: HttpResponseOptions::default(),
        }
    }

    /// Creates a response from backend response and request-scoped options.
    pub(crate) fn from_backend(
        meta: HttpResponseMeta,
        backend: reqwest::Response,
        read_timeout: Duration,
        cancellation_token: Option<CancellationToken>,
        request_url: Url,
        options: HttpResponseOptions,
    ) -> Self {
        Self {
            meta,
            backend: Some(backend),
            buffered_body: None,
            runtime: HttpResponseRuntime::new(
                read_timeout,
                cancellation_token,
                request_url,
            ),
            options,
        }
    }

    /// Returns shared response metadata.
    #[inline(always)]
    pub fn meta(&self) -> &HttpResponseMeta {
        &self.meta
    }

    /// Returns response status code.
    #[inline(always)]
    pub fn status(&self) -> StatusCode {
        self.meta.status()
    }

    /// Returns response headers.
    #[inline(always)]
    pub fn headers(&self) -> &HeaderMap {
        self.meta.headers()
    }

    /// Returns final response URL.
    #[inline(always)]
    pub fn url(&self) -> &Url {
        self.meta.url()
    }

    /// Returns request URL used in response read context.
    #[inline(always)]
    pub fn request_url(&self) -> &Url {
        &self.runtime.request_url
    }

    /// Returns whether status is success.
    #[inline(always)]
    pub fn is_success(&self) -> bool {
        self.status().is_success()
    }

    /// Returns parsed `Retry-After` hint when status and headers provide one.
    #[inline(always)]
    pub fn retry_after_hint(&self) -> Option<Duration> {
        self.meta.retry_after_hint()
    }

    /// Returns a previous body read failure, if any.
    ///
    /// # Returns
    /// `Some(HttpError)` when an earlier body read failed; otherwise `None`.
    fn previous_body_read_error(&self) -> Option<HttpError> {
        let guard = self.runtime.body_read_failure.lock().ok()?;
        guard.as_ref().map(BodyReadFailure::to_error)
    }

    /// Stores the first body read failure for later read attempts.
    ///
    /// # Parameters
    /// - `error`: Error produced while reading the response body.
    fn remember_body_read_failure(&self, error: &HttpError) {
        Self::remember_body_read_failure_state(
            &self.runtime.body_read_failure,
            error,
        );
    }

    /// Stores the first body read failure in a shared state holder.
    ///
    /// # Parameters
    /// - `state`: Shared failure state captured by response streams.
    /// - `error`: Error produced while reading the response body.
    fn remember_body_read_failure_state(
        state: &BodyReadFailureState,
        error: &HttpError,
    ) {
        if let Ok(mut guard) = state.lock() {
            guard.get_or_insert_with(|| BodyReadFailure::from_error(error));
        }
    }

    /// Returns `Ok(self)` for success statuses, otherwise maps a status error
    /// with `Retry-After` and response-body preview context.
    pub(crate) async fn into_success_or_status_error(
        self,
        message_prefix: &str,
    ) -> HttpResult<Self> {
        let status = self.status();
        if status.is_success() {
            return Ok(self);
        }
        let retry_after = self.retry_after_hint();
        let method = self.meta.method().clone();
        let url = self.request_url().clone();
        let error_preview_limit = self.options.error_response_preview_limit;
        let log_redactor = self.log_redactor().clone();
        let body_preview =
            self.into_error_body_preview(error_preview_limit).await?;
        let message = format!(
            "{} with status {} for {} {}; response body preview: {}",
            message_prefix,
            status,
            method,
            log_redactor.redact_url(&url),
            body_preview
        );
        let mut mapped = HttpError::status(status, message)
            .with_method(&method)
            .with_url(&url)
            .with_response_body_preview(body_preview)
            .with_log_redactor(log_redactor.clone());
        if let Some(retry_after) = retry_after {
            mapped = mapped.with_retry_after(retry_after);
        }
        Err(mapped)
    }

    /// Consumes this response and returns a bounded body preview for status
    /// errors.
    ///
    /// # Errors
    /// Returns [`HttpErrorKind::Cancelled`](crate::HttpErrorKind::Cancelled)
    /// when the request cancellation token fires while preview bytes are being
    /// read.
    pub(crate) async fn into_error_body_preview(
        mut self,
        max_bytes: usize,
    ) -> HttpResult<String> {
        let limit = max_bytes.max(1);
        let Some(backend) = self.backend.take() else {
            return Ok("<empty>".to_string());
        };
        let content_type = Self::content_type_header(self.meta.headers());
        self.read_error_body_preview(backend, limit, content_type)
            .await
    }

    /// Returns full body bytes, consuming backend stream lazily on first call.
    ///
    /// # Errors
    ///
    /// Returns [`HttpErrorKind::Other`](crate::HttpErrorKind::Other) when the
    /// response body exceeds the configured aggregation limit.
    pub async fn bytes(&mut self) -> HttpResult<Bytes> {
        let body_limit = self.options.response_body_size_limit;
        if let Some(body) = &self.buffered_body {
            if body.len() > body_limit {
                return Err(self.response_body_size_limit_error(body.len()));
            }
            return Ok(body.clone());
        }
        if let Some(error) = self.previous_body_read_error() {
            return Err(error);
        }
        let Some(mut backend) = self.backend.take() else {
            self.buffered_body = Some(Bytes::new());
            return Ok(Bytes::new());
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
                let observed_size =
                    usize::try_from(content_length).unwrap_or(usize::MAX);
                let error = self.response_body_size_limit_error(observed_size);
                self.remember_body_read_failure(&error);
                return Err(error);
            }
        }
        let mut body = bytes::BytesMut::new();
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
                        let observed_size =
                            error.checked_attempted().unwrap_or(usize::MAX);
                        let error =
                            self.response_body_size_limit_error(observed_size);
                        self.remember_body_read_failure(&error);
                        return Err(error);
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(Ok(None)) => {
                    let body = body.freeze();
                    self.buffered_body = Some(body.clone());
                    return Ok(body);
                }
                Ok(Err(error)) => {
                    let error = map_response_read_error(
                        error,
                        method,
                        url,
                        status,
                        self.log_redactor(),
                    );
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

    /// Returns body as stream; if already buffered, returns stream backed by
    /// cached bytes.
    pub fn stream(&mut self) -> HttpResult<HttpByteStream> {
        if let Some(body) = self.buffered_body.as_ref() {
            let bytes = body.clone();
            return Ok(Box::pin(futures_stream::once(
                async move { Ok(bytes) },
            )));
        }
        if let Some(error) = self.previous_body_read_error() {
            return Err(error);
        }
        if let Some(error) = self.cancelled_error_if_needed(
            "Streaming response cancelled before reading response body",
        ) {
            return Err(error);
        }
        let Some(backend) = self.backend.take() else {
            return Ok(Box::pin(futures_stream::empty()));
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
                            Self::remember_body_read_failure_state(&body_read_failure, &error);
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
                        Self::remember_body_read_failure_state(&body_read_failure, &mapped);
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
                        Self::remember_body_read_failure_state(&body_read_failure, &error);
                        yield Err(error);
                        break;
                    }
                }
            }
        };
        Ok(Box::pin(wrapped))
    }

    /// Interprets response body as UTF-8 text.
    pub async fn text(&mut self) -> HttpResult<String> {
        let body = self.bytes().await?;
        String::from_utf8(body.to_vec()).map_err(|error| {
            HttpError::decode(format!(
                "Failed to decode response body as UTF-8: {}",
                error
            ))
            .with_status(self.meta.status())
            .with_url(self.meta.url())
            .with_log_redactor(self.log_redactor().clone())
        })
    }

    /// Deserializes response body as JSON.
    pub async fn json<T>(&mut self) -> HttpResult<T>
    where
        T: DeserializeOwned,
    {
        let body = self.bytes().await?;
        LenientJsonDecoder::new(JsonDecodeOptions::strict())
            .decode_slice(&body)
            .map_err(|error| {
                HttpError::decode("Failed to decode response JSON")
                    .with_status(self.meta.status())
                    .with_url(self.meta.url())
                    .with_source(error)
                    .with_log_redactor(self.log_redactor().clone())
            })
    }

    /// Overrides the maximum allowed size (in bytes) for one SSE line on this
    /// response.
    ///
    /// Values below 1 are clamped to 1. Returns `self` so callers can chain
    /// configuration before consuming the body with [`Self::sse_messages`]
    /// or [`Self::sse_chunks`] (together with [`Self::sse_json_mode`],
    /// [`Self::sse_done_marker_policy`], etc.).
    #[inline(always)]
    pub fn sse_max_line_bytes(mut self, max_line_bytes: usize) -> Self {
        self.options.sse_max_line_bytes = max_line_bytes.max(1);
        self
    }

    /// Overrides the maximum allowed size (in bytes) for one SSE frame on this
    /// response.
    ///
    /// Values below 1 are clamped to 1. Returns `self` for chained
    /// configuration.
    #[inline(always)]
    pub fn sse_max_frame_bytes(mut self, max_frame_bytes: usize) -> Self {
        self.options.sse_max_frame_bytes = max_frame_bytes.max(1);
        self
    }

    /// Overrides the JSON decoding mode used by [`Self::sse_chunks`] on this
    /// response.
    #[inline(always)]
    pub fn sse_json_mode(mut self, mode: SseJsonMode) -> Self {
        self.options.sse_json_mode = mode;
        self
    }

    /// Overrides how [`Self::sse_chunks`] detects end-of-stream from trimmed
    /// `data:` payloads.
    #[inline(always)]
    pub fn sse_done_marker_policy(mut self, policy: DoneMarkerPolicy) -> Self {
        self.options.sse_done_marker_policy = policy;
        self
    }

    /// Decodes body stream as SSE messages using this response's SSE line/frame
    /// byte limits (from client defaults unless overridden via
    /// [`Self::sse_max_line_bytes`] / [`Self::sse_max_frame_bytes`]).
    pub fn sse_messages(mut self) -> SseMessageStream {
        let max_line_bytes = self.options.sse_max_line_bytes;
        let max_frame_bytes = self.options.sse_max_frame_bytes;
        let log_redactor = self.log_redactor().clone();
        let decoded: SseMessageStream = match self.stream() {
            Ok(stream) => crate::sse::decode_messages_from_stream_with_limits(
                stream,
                max_line_bytes,
                max_frame_bytes,
            ),
            Err(error) => {
                Box::pin(futures_stream::once(async move { Err(error) }))
            }
        };
        Box::pin(decoded.map(move |result| {
            result
                .map_err(|error| error.with_log_redactor(log_redactor.clone()))
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
        let decoded: crate::sse::SseRecordStream = match self.stream() {
            Ok(stream) => crate::sse::decode_records_from_stream_with_limits(
                stream,
                max_line_bytes,
                max_frame_bytes,
            ),
            Err(error) => {
                Box::pin(futures_stream::once(async move { Err(error) }))
            }
        };
        Box::pin(decoded.map(move |result| {
            result
                .map_err(|error| error.with_log_redactor(log_redactor.clone()))
        }))
    }

    /// Decodes SSE `data:` lines as JSON chunks using this response's SSE JSON
    /// mode, done-marker policy, and line/frame limits (see
    /// [`Self::sse_json_mode`], [`Self::sse_done_marker_policy`],
    /// [`Self::sse_max_line_bytes`], [`Self::sse_max_frame_bytes`]).
    pub fn sse_chunks<T>(mut self) -> SseChunkStream<T>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let done_policy = self.options.sse_done_marker_policy.clone();
        let mode = self.options.sse_json_mode;
        let max_line_bytes = self.options.sse_max_line_bytes;
        let max_frame_bytes = self.options.sse_max_frame_bytes;
        let log_redactor = self.log_redactor().clone();
        let decoded: SseChunkStream<T> = match self.stream() {
            Ok(stream) => {
                crate::sse::decode_json_chunks_from_stream_with_limits(
                    stream,
                    done_policy,
                    mode,
                    max_line_bytes,
                    max_frame_bytes,
                )
            }
            Err(error) => {
                Box::pin(futures_stream::once(async move { Err(error) }))
            }
        };
        Box::pin(decoded.map(move |result| {
            result
                .map_err(|error| error.with_log_redactor(log_redactor.clone()))
        }))
    }

    /// Returns the shared log redactor used for response diagnostics and
    /// errors.
    #[inline(always)]
    pub(crate) fn log_redactor(&self) -> &HttpRedactor {
        &self.options.log_redactor
    }

    /// Returns a buffered body reference for response logging if available.
    ///
    /// # Returns
    /// `Some(&Bytes)` when response body has already been buffered.
    #[inline(always)]
    pub(crate) fn buffered_body_for_logging(&self) -> Option<&Bytes> {
        self.buffered_body.as_ref()
    }

    /// Returns whether logger may safely buffer the full body for logging.
    ///
    /// # Parameters
    /// - `body_log_limit`: Configured logging body preview limit in bytes.
    ///
    /// # Returns
    /// `true` only when this response is not SSE, has an explicit
    /// `Content-Length`, and declared length is within `body_log_limit`.
    #[inline]
    pub(crate) fn can_buffer_body_for_logging(
        &self,
        body_log_limit: usize,
    ) -> bool {
        if self.backend.is_none() {
            return false;
        }
        if self.is_sse_response() {
            return false;
        }
        self.content_length_hint().is_some_and(|content_length| {
            content_length <= body_log_limit as u64
        })
    }

    /// Reads bounded preview bytes from a response body for status error
    /// messages.
    ///
    /// # Errors
    /// Returns [`HttpErrorKind::Cancelled`](crate::HttpErrorKind::Cancelled)
    /// when the supplied cancellation token fires while waiting for preview
    /// bytes.
    async fn read_error_body_preview(
        &self,
        mut response: reqwest::Response,
        max_bytes: usize,
        content_type: Option<HeaderValue>,
    ) -> HttpResult<String> {
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
        let mut truncated = false;
        let capture_limit = limit.saturating_add(1);
        let mut capture_budget = ResourceBudget::new(
            "status error response body preview",
            capture_limit,
        );

        loop {
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
                    item = tokio::time::timeout(read_timeout, response.chunk()) => item,
                }
            } else {
                tokio::time::timeout(read_timeout, response.chunk()).await
            };
            match next {
                Ok(Ok(Some(chunk))) => {
                    let captured =
                        capture_budget.consume_available(chunk.len());
                    preview.extend_from_slice(&chunk[..captured]);
                    if captured < chunk.len() {
                        truncated = true;
                        break;
                    }
                    if preview.len() > limit {
                        truncated = true;
                        break;
                    }
                }
                Ok(Ok(None)) => break,
                Ok(Err(error)) => {
                    let error = error.without_url();
                    return Ok(format!(
                        "<error body unavailable: failed to read response body: {}>",
                        error
                    ));
                }
                Err(_) => {
                    return Ok(format!(
                        "<error body unavailable: read timeout after {:?}>",
                        read_timeout
                    ));
                }
            }
        }
        if preview.len() > limit {
            preview.truncate(limit);
            truncated = true;
        }
        Ok(Self::render_error_body_preview(
            &preview,
            source_len,
            truncated,
            content_type.as_ref(),
            &self.options.log_redactor,
        ))
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
            .is_some_and(CancellationToken::is_cancelled)
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
        self.meta
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
    }

    /// Returns a response-body aggregation limit error with request context.
    fn response_body_size_limit_error(
        &self,
        observed_size: usize,
    ) -> HttpError {
        let limit = self.options.response_body_size_limit;
        HttpError::other(format!(
            "Response body exceeds configured limit of {limit} bytes (observed {observed_size} bytes)"
        ))
        .with_method(self.meta.method())
        .with_url(&self.runtime.request_url)
        .with_status(self.meta.status())
        .with_log_redactor(self.log_redactor().clone())
    }

    /// Returns whether response content-type is SSE (`text/event-stream`).
    fn is_sse_response(&self) -> bool {
        self.meta
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(content_type::is_sse)
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
        log_redactor: &HttpRedactor,
    ) -> String {
        if bytes.is_empty() && !truncated {
            return "<empty>".to_owned();
        }
        let capture = if truncated {
            source_len.map_or_else(
                || qubit_redact::http::BodyCapture::truncated_unknown(bytes),
                |total_len| {
                    qubit_redact::http::BodyCapture::truncated(
                        bytes,
                        Some(total_len),
                    )
                    .unwrap_or_else(|_| {
                        qubit_redact::http::BodyCapture::truncated_unknown(
                            bytes,
                        )
                    })
                },
            )
        } else {
            qubit_redact::http::BodyCapture::complete(bytes)
        };
        log_redactor.redact_body(capture, content_type).to_string()
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
