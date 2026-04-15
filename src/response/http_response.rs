/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Unified HTTP response type and helpers.

use std::time::Duration;

use async_stream::stream;
use bytes::Bytes;
use futures_util::{stream as futures_stream, StreamExt};
use http::{HeaderMap, Method, StatusCode};
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::constants::{
    DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES, DEFAULT_SSE_MAX_FRAME_BYTES,
    DEFAULT_SSE_MAX_LINE_BYTES,
};
use crate::error::backend_error_mapper::{map_reqwest_error, ReqwestErrorPhase};
use crate::sse::{DoneMarkerPolicy, SseChunkStream, SseEventStream, SseJsonMode};
use crate::{HttpByteStream, HttpError, HttpErrorKind, HttpResult};

use super::HttpResponseMeta;

/// Runtime state bound to one response instance.
#[derive(Debug, Clone)]
struct HttpResponseRuntime {
    /// Per-response read timeout inherited from request/client.
    read_timeout: Duration,
    /// Optional cancellation token inherited from request.
    cancellation_token: Option<CancellationToken>,
    /// Request URL used in read/cancellation error context.
    request_url: Url,
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
        }
    }
}

/// Decode/error-preview options bound to one response instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpResponseOptions {
    /// Maximum bytes captured for status-error body preview.
    pub error_response_preview_limit: usize,
    /// Default JSON decoding mode used by stream JSON helpers.
    pub sse_json_mode: SseJsonMode,
    /// Default maximum bytes allowed for one SSE line.
    pub sse_max_line_bytes: usize,
    /// Default maximum bytes allowed for one SSE frame.
    pub sse_max_frame_bytes: usize,
    /// How [`crate::HttpResponse::sse_chunks`] recognizes end-of-stream `data:` markers.
    pub sse_done_marker_policy: DoneMarkerPolicy,
}

impl Default for HttpResponseOptions {
    fn default() -> Self {
        Self {
            error_response_preview_limit: DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES,
            sse_json_mode: SseJsonMode::Lenient,
            sse_max_line_bytes: DEFAULT_SSE_MAX_LINE_BYTES,
            sse_max_frame_bytes: DEFAULT_SSE_MAX_FRAME_BYTES,
            sse_done_marker_policy: DoneMarkerPolicy::default(),
        }
    }
}

impl HttpResponseOptions {
    pub(crate) fn new(
        error_response_preview_limit: usize,
        sse_json_mode: SseJsonMode,
        sse_max_line_bytes: usize,
        sse_max_frame_bytes: usize,
        sse_done_marker_policy: DoneMarkerPolicy,
    ) -> Self {
        Self {
            error_response_preview_limit: error_response_preview_limit.max(1),
            sse_json_mode,
            sse_max_line_bytes: sse_max_line_bytes.max(1),
            sse_max_frame_bytes: sse_max_frame_bytes.max(1),
            sse_done_marker_policy,
        }
    }
}

/// Unified HTTP response with lazily consumed body.
#[derive(Debug)]
pub struct HttpResponse {
    /// Response metadata (status, headers, final URL, request method).
    pub meta: HttpResponseMeta,
    /// Raw backend response until consumed.
    backend: Option<reqwest::Response>,
    /// Cached full body bytes after eager or lazy read.
    buffered_body: Option<Bytes>,
    /// Runtime state inherited from request/client.
    runtime: HttpResponseRuntime,
    /// Decode and error-preview options inherited from client options.
    options: HttpResponseOptions,
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
            runtime: HttpResponseRuntime::new(Duration::from_secs(30), None, url),
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
            runtime: HttpResponseRuntime::new(read_timeout, cancellation_token, request_url),
            options,
        }
    }

    /// Returns shared response metadata.
    #[inline]
    pub fn meta(&self) -> &HttpResponseMeta {
        &self.meta
    }

    /// Returns response status code.
    #[inline]
    pub fn status(&self) -> StatusCode {
        self.meta.status
    }

    /// Returns response headers.
    #[inline]
    pub fn headers(&self) -> &HeaderMap {
        &self.meta.headers
    }

    /// Returns final response URL.
    #[inline]
    pub fn url(&self) -> &Url {
        &self.meta.url
    }

    /// Returns request URL used in response read context.
    #[inline]
    pub fn request_url(&self) -> &Url {
        &self.runtime.request_url
    }

    /// Returns whether status is success.
    #[inline]
    pub fn is_success(&self) -> bool {
        self.status().is_success()
    }

    /// Returns parsed `Retry-After` hint when status and headers provide one.
    #[inline]
    pub fn retry_after_hint(&self) -> Option<Duration> {
        self.meta.retry_after_hint()
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
        let method = self.meta.method.clone();
        let url = self.request_url().clone();
        let error_preview_limit = self.options.error_response_preview_limit;
        let body_preview = self.into_error_body_preview(error_preview_limit).await;
        let message = format!(
            "{} with status {} for {} {}; response body preview: {}",
            message_prefix, status, method, url, body_preview
        );
        let mut mapped = HttpError::status(status, message)
            .with_method(&method)
            .with_url(&url)
            .with_response_body_preview(body_preview);
        if let Some(retry_after) = retry_after {
            mapped = mapped.with_retry_after(retry_after);
        }
        Err(mapped)
    }

    /// Consumes this response and returns a bounded body preview for status errors.
    pub(crate) async fn into_error_body_preview(mut self, max_bytes: usize) -> String {
        let limit = max_bytes.max(1);
        if let Some(body) = self.buffered_body.take() {
            let end = body.len().min(limit);
            return Self::render_error_body_preview(&body[..end], body.len() > limit);
        }
        let Some(backend) = self.backend.take() else {
            return "<empty>".to_string();
        };
        Self::read_error_body_preview(backend, self.runtime.read_timeout, limit).await
    }

    /// Returns full body bytes, consuming backend stream lazily on first call.
    pub async fn bytes(&mut self) -> HttpResult<Bytes> {
        if let Some(body) = &self.buffered_body {
            return Ok(body.clone());
        }
        let Some(backend) = self.backend.take() else {
            self.buffered_body = Some(Bytes::new());
            return Ok(Bytes::new());
        };

        let method = self.meta.method.clone();
        let read_future = tokio::time::timeout(self.runtime.read_timeout, backend.bytes());
        let next = if let Some(token) = &self.runtime.cancellation_token {
            tokio::select! {
                _ = token.cancelled() => {
                    return Err(HttpError::cancelled("Request cancelled while reading response body")
                        .with_method(&method)
                        .with_url(&self.runtime.request_url));
                }
                read_result = read_future => read_result,
            }
        } else {
            read_future.await
        };

        match next {
            Ok(Ok(body)) => {
                self.buffered_body = Some(body.clone());
                Ok(body)
            }
            Ok(Err(error)) => Err(map_reqwest_error(
                error,
                HttpErrorKind::Decode,
                Some(ReqwestErrorPhase::Read),
                Some(method),
                Some(self.runtime.request_url.clone()),
            )),
            Err(_) => Err(HttpError::read_timeout(format!(
                "Read timeout after {:?} while reading response body",
                self.runtime.read_timeout
            ))
            .with_method(&self.meta.method)
            .with_url(&self.runtime.request_url)),
        }
    }

    /// Returns body as stream; if already buffered, returns stream backed by cached bytes.
    pub fn stream(&mut self) -> HttpResult<HttpByteStream> {
        if let Some(body) = self.buffered_body.as_ref() {
            let bytes = body.clone();
            return Ok(Box::pin(futures_stream::once(async move { Ok(bytes) })));
        }
        let Some(backend) = self.backend.take() else {
            return Ok(Box::pin(futures_stream::empty()));
        };

        let method = self.meta.method.clone();
        let url = self.runtime.request_url.clone();
        let read_timeout = self.runtime.read_timeout;
        let cancellation_token = self.runtime.cancellation_token.clone();
        let mut stream = backend.bytes_stream();
        let wrapped = stream! {
            loop {
                let next = if let Some(token) = &cancellation_token {
                    tokio::select! {
                        _ = token.cancelled() => {
                            yield Err(HttpError::cancelled("Streaming response cancelled while reading body")
                                .with_method(&method)
                                .with_url(&url));
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
                        let mapped = map_reqwest_error(
                            error,
                            HttpErrorKind::Transport,
                            Some(ReqwestErrorPhase::Read),
                            Some(method.clone()),
                            Some(url.clone()),
                        );
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
                        .with_url(&url);
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
            .with_status(self.meta.status)
            .with_url(&self.meta.url)
        })
    }

    /// Deserializes response body as JSON.
    pub async fn json<T>(&mut self) -> HttpResult<T>
    where
        T: DeserializeOwned,
    {
        let body = self.bytes().await?;
        serde_json::from_slice(&body).map_err(|error| {
            HttpError::decode(format!("Failed to decode response JSON: {}", error))
                .with_status(self.meta.status)
                .with_url(&self.meta.url)
        })
    }

    /// Overrides the maximum allowed size (in bytes) for one SSE line on this response.
    ///
    /// Values below 1 are clamped to 1. Returns `self` so callers can chain configuration
    /// before consuming the body with [`Self::sse_events`] or [`Self::sse_chunks`]
    /// (together with [`Self::sse_json_mode`], [`Self::sse_done_marker_policy`], etc.).
    #[inline]
    pub fn sse_max_line_bytes(mut self, max_line_bytes: usize) -> Self {
        self.options.sse_max_line_bytes = max_line_bytes.max(1);
        self
    }

    /// Overrides the maximum allowed size (in bytes) for one SSE frame on this response.
    ///
    /// Values below 1 are clamped to 1. Returns `self` for chained configuration.
    #[inline]
    pub fn sse_max_frame_bytes(mut self, max_frame_bytes: usize) -> Self {
        self.options.sse_max_frame_bytes = max_frame_bytes.max(1);
        self
    }

    /// Overrides the JSON decoding mode used by [`Self::sse_chunks`] on this response.
    #[inline]
    pub fn sse_json_mode(mut self, mode: SseJsonMode) -> Self {
        self.options.sse_json_mode = mode;
        self
    }

    /// Overrides how [`Self::sse_chunks`] detects end-of-stream from trimmed `data:` payloads.
    #[inline]
    pub fn sse_done_marker_policy(mut self, policy: DoneMarkerPolicy) -> Self {
        self.options.sse_done_marker_policy = policy;
        self
    }

    /// Decodes body stream as SSE events using this response's SSE line/frame byte limits (from
    /// client defaults unless overridden via [`Self::sse_max_line_bytes`] /
    /// [`Self::sse_max_frame_bytes`]).
    pub fn sse_events(mut self) -> SseEventStream {
        let max_line_bytes = self.options.sse_max_line_bytes;
        let max_frame_bytes = self.options.sse_max_frame_bytes;
        match self.stream() {
            Ok(stream) => crate::sse::decode_events_from_stream_with_limits(
                stream,
                max_line_bytes,
                max_frame_bytes,
            ),
            Err(error) => Box::pin(futures_stream::once(async move { Err(error) })),
        }
    }

    /// Decodes SSE `data:` lines as JSON chunks using this response's SSE JSON mode, done-marker
    /// policy, and line/frame limits (see [`Self::sse_json_mode`], [`Self::sse_done_marker_policy`],
    /// [`Self::sse_max_line_bytes`], [`Self::sse_max_frame_bytes`]).
    pub fn sse_chunks<T>(mut self) -> SseChunkStream<T>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let done_policy = self.options.sse_done_marker_policy.clone();
        let mode = self.options.sse_json_mode;
        let max_line_bytes = self.options.sse_max_line_bytes;
        let max_frame_bytes = self.options.sse_max_frame_bytes;
        match self.stream() {
            Ok(stream) => crate::sse::decode_json_chunks_from_stream_with_limits(
                stream,
                done_policy,
                mode,
                max_line_bytes,
                max_frame_bytes,
            ),
            Err(error) => Box::pin(futures_stream::once(async move { Err(error) })),
        }
    }

    /// Reads bounded preview bytes from a response body for status error messages.
    async fn read_error_body_preview(
        mut response: reqwest::Response,
        read_timeout: Duration,
        max_bytes: usize,
    ) -> String {
        let limit = max_bytes.max(1);
        let mut preview = Vec::new();
        let mut truncated = false;

        loop {
            let next = tokio::time::timeout(read_timeout, response.chunk()).await;
            match next {
                Ok(Ok(Some(chunk))) => {
                    if preview.len() >= limit {
                        truncated = true;
                        break;
                    }
                    let remaining = limit - preview.len();
                    if chunk.len() > remaining {
                        preview.extend_from_slice(&chunk[..remaining]);
                        truncated = true;
                        break;
                    }
                    preview.extend_from_slice(&chunk);
                }
                Ok(Ok(None)) => break,
                Ok(Err(error)) => {
                    return format!(
                        "<error body unavailable: failed to read response body: {}>",
                        error
                    );
                }
                Err(_) => {
                    return format!(
                        "<error body unavailable: read timeout after {:?}>",
                        read_timeout
                    );
                }
            }
        }
        Self::render_error_body_preview(&preview, truncated)
    }

    fn render_error_body_preview(bytes: &[u8], truncated: bool) -> String {
        if bytes.is_empty() {
            return "<empty>".to_string();
        }
        let suffix = if truncated { "...<truncated>" } else { "" };
        match std::str::from_utf8(bytes) {
            Ok(text) => format!("{text}{suffix}"),
            Err(_) => format!("<binary {} bytes>{suffix}", bytes.len()),
        }
    }
}
