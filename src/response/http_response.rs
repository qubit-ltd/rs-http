// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unified HTTP response type and helpers.
use std::fmt;
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream as futures_stream;
use http::HeaderMap;
use http::Method;
use http::StatusCode;
use http::header::CONTENT_LENGTH;
use http::header::CONTENT_TYPE;
use qubit_budget::json::JsonValueLimits;
use qubit_redact::Redactor;
use serde::de::DeserializeOwned;
use url::Url;

use super::HttpResponseMeta;
use super::HttpResponseOptions;
use super::internal::HttpResponseBodyState;
use super::internal::http_response_runtime::HttpResponseRuntime;
use super::internal::http_response_runtime::previous_body_read_error;
use super::internal::http_response_runtime::remember_body_read_failure_state;
use crate::HttpCancellationToken;
use crate::HttpError;
use crate::HttpErrorKind;
use crate::HttpResult;
use crate::content_type;
use crate::error::ReqwestErrorPhase;
use crate::error::backend_error_mapper::map_reqwest_error;
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

// Implements bounded aggregate body reads and text/JSON decoding.
mod aggregate;
// Implements one-shot backend streaming and cached-body streams.
mod streaming;
// Implements bounded non-success response capture and diagnostics.
mod status_capture;

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
