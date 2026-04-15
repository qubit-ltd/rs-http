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

use crate::client::error_mapper::{map_reqwest_error, ReqwestErrorPhase};
use crate::sse::{DoneMarkerPolicy, SseChunkStream, SseEventStream, SseJsonMode};
use crate::{HttpByteStream, HttpError, HttpErrorKind, HttpResult, SseDecodeOptions};

use super::HttpResponseMeta;

/// Unified HTTP response with lazily consumed body.
#[derive(Debug)]
pub struct HttpResponse {
    /// Response metadata (status, headers, final URL, request method).
    pub meta: HttpResponseMeta,
    /// Raw backend response until consumed.
    backend: Option<reqwest::Response>,
    /// Cached full body bytes after eager or lazy read.
    buffered_body: Option<Bytes>,
    /// Per-response read timeout inherited from request/client.
    read_timeout: Duration,
    /// Optional cancellation token inherited from request.
    cancellation_token: Option<CancellationToken>,
    /// Request URL used in read/cancellation error context.
    request_url: Url,
    /// Default SSE decode options inherited from client options.
    sse_decode_options: SseDecodeOptions,
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
            read_timeout: Duration::from_secs(30),
            cancellation_token: None,
            request_url: url,
            sse_decode_options: SseDecodeOptions::default(),
        }
    }

    /// Creates a response from backend response and request-scoped options.
    pub(crate) fn from_backend(
        meta: HttpResponseMeta,
        backend: reqwest::Response,
        read_timeout: Duration,
        cancellation_token: Option<CancellationToken>,
        request_url: Url,
        sse_decode_options: SseDecodeOptions,
    ) -> Self {
        Self {
            meta,
            backend: Some(backend),
            buffered_body: None,
            read_timeout,
            cancellation_token,
            request_url,
            sse_decode_options,
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
        &self.request_url
    }

    /// Returns cached full body if already buffered.
    #[inline]
    pub fn buffered_body(&self) -> Option<&Bytes> {
        self.buffered_body.as_ref()
    }

    /// Returns whether status is success.
    #[inline]
    pub fn is_success(&self) -> bool {
        self.status().is_success()
    }

    /// Returns full body bytes, consuming backend stream lazily on first call.
    pub async fn bytes_body(&mut self) -> HttpResult<Bytes> {
        if let Some(body) = &self.buffered_body {
            return Ok(body.clone());
        }
        let Some(backend) = self.backend.take() else {
            self.buffered_body = Some(Bytes::new());
            return Ok(Bytes::new());
        };

        let method = self.meta.method.clone();
        let read_future = tokio::time::timeout(self.read_timeout, backend.bytes());
        let next = if let Some(token) = &self.cancellation_token {
            tokio::select! {
                _ = token.cancelled() => {
                    return Err(HttpError::cancelled("Request cancelled while reading response body")
                        .with_method(&method)
                        .with_url(&self.request_url));
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
                Some(self.request_url.clone()),
            )),
            Err(_) => Err(HttpError::read_timeout(format!(
                "Read timeout after {:?} while reading response body",
                self.read_timeout
            ))
            .with_method(&self.meta.method)
            .with_url(&self.request_url)),
        }
    }

    /// Returns body as stream; if already buffered, returns stream backed by cached bytes.
    pub fn stream_body(&mut self) -> HttpResult<HttpByteStream> {
        if let Some(body) = self.buffered_body.as_ref() {
            let bytes = body.clone();
            return Ok(Box::pin(futures_stream::once(async move { Ok(bytes) })));
        }
        let Some(backend) = self.backend.take() else {
            return Ok(Box::pin(futures_stream::empty()));
        };

        let method = self.meta.method.clone();
        let url = self.request_url.clone();
        let read_timeout = self.read_timeout;
        let cancellation_token = self.cancellation_token.clone();
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
        let body = self.bytes_body().await?;
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
        let body = self.bytes_body().await?;
        serde_json::from_slice(&body).map_err(|error| {
            HttpError::decode(format!("Failed to decode response JSON: {}", error))
                .with_status(self.meta.status)
                .with_url(&self.meta.url)
        })
    }

    /// Decodes body stream as SSE events with default limits.
    pub fn decode_sse_events(self) -> SseEventStream {
        let options = self.sse_decode_options;
        self.decode_sse_events_with_limits(options.max_line_bytes, options.max_frame_bytes)
    }

    /// Decodes body stream as SSE events with explicit limits.
    pub fn decode_sse_events_with_limits(
        mut self,
        max_line_bytes: usize,
        max_frame_bytes: usize,
    ) -> SseEventStream {
        match self.stream_body() {
            Ok(stream) => crate::sse::decode_events_from_stream_with_limits(
                stream,
                max_line_bytes,
                max_frame_bytes,
            ),
            Err(error) => Box::pin(futures_stream::once(async move { Err(error) })),
        }
    }

    /// Decodes SSE data chunks as JSON using response default options.
    pub fn decode_sse_json_chunks<T>(self, done_policy: DoneMarkerPolicy) -> SseChunkStream<T>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let options = self.sse_decode_options;
        self.decode_sse_json_chunks_with_mode_and_limits(
            done_policy,
            options.json_mode,
            options.max_line_bytes,
            options.max_frame_bytes,
        )
    }

    /// Decodes SSE JSON chunks with explicit mode and default limits.
    pub fn decode_sse_json_chunks_with_mode<T>(
        self,
        done_policy: DoneMarkerPolicy,
        mode: SseJsonMode,
    ) -> SseChunkStream<T>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let options = self.sse_decode_options;
        self.decode_sse_json_chunks_with_mode_and_limits(
            done_policy,
            mode,
            options.max_line_bytes,
            options.max_frame_bytes,
        )
    }

    /// Decodes SSE JSON chunks with explicit mode and limits.
    pub fn decode_sse_json_chunks_with_mode_and_limits<T>(
        mut self,
        done_policy: DoneMarkerPolicy,
        mode: SseJsonMode,
        max_line_bytes: usize,
        max_frame_bytes: usize,
    ) -> SseChunkStream<T>
    where
        T: DeserializeOwned + Send + 'static,
    {
        match self.stream_body() {
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
    pub(crate) async fn read_error_body_preview(
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

