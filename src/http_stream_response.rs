/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Streaming HTTP response wrapper.

use http::{HeaderMap, StatusCode};
use serde::de::DeserializeOwned;
use url::Url;

use crate::{
    constants::{DEFAULT_SSE_MAX_FRAME_BYTES, DEFAULT_SSE_MAX_LINE_BYTES},
    sse::{DoneMarkerPolicy, SseChunkStream, SseEventStream, SseJsonMode},
    HttpByteStream,
};

/// HTTP response metadata plus a lazy body stream (from [`crate::HttpClient::execute_stream`]).
pub struct HttpStreamResponse {
    /// HTTP status code of the first response line.
    pub status: StatusCode,
    /// Response headers available before consuming the body.
    pub headers: HeaderMap,
    /// Effective URL after redirects.
    pub url: Url,
    /// Body as an async stream of [`bytes::Bytes`] chunks.
    stream: HttpByteStream,
    /// Default JSON decoding mode used by [`HttpStreamResponse::decode_json_chunks`].
    sse_json_mode: SseJsonMode,
    /// Default maximum bytes allowed for one SSE line.
    sse_max_line_bytes: usize,
    /// Default maximum bytes allowed for one SSE frame.
    sse_max_frame_bytes: usize,
}

impl HttpStreamResponse {
    /// Wraps status, headers, URL, and the byte stream.
    ///
    /// # Parameters
    /// - `status`: HTTP status.
    /// - `headers`: Header map.
    /// - `url`: Final URL.
    /// - `stream`: Pinned body stream.
    ///
    /// # Returns
    /// New [`HttpStreamResponse`].
    pub fn new(status: StatusCode, headers: HeaderMap, url: Url, stream: HttpByteStream) -> Self {
        Self::new_with_sse_options(
            status,
            headers,
            url,
            stream,
            SseJsonMode::Lenient,
            DEFAULT_SSE_MAX_LINE_BYTES,
            DEFAULT_SSE_MAX_FRAME_BYTES,
        )
    }

    /// Wraps status, headers, URL, and byte stream with SSE decode defaults.
    ///
    /// # Parameters
    /// - `status`: HTTP status.
    /// - `headers`: Header map.
    /// - `url`: Final URL.
    /// - `stream`: Pinned body stream.
    /// - `sse_json_mode`: Default JSON strictness used by `decode_json_chunks`.
    /// - `sse_max_line_bytes`: Default max bytes for one SSE line.
    /// - `sse_max_frame_bytes`: Default max bytes for one SSE frame.
    ///
    /// # Returns
    /// New [`HttpStreamResponse`].
    pub(crate) fn new_with_sse_options(
        status: StatusCode,
        headers: HeaderMap,
        url: Url,
        stream: HttpByteStream,
        sse_json_mode: SseJsonMode,
        sse_max_line_bytes: usize,
        sse_max_frame_bytes: usize,
    ) -> Self {
        Self {
            status,
            headers,
            url,
            stream,
            sse_json_mode,
            sse_max_line_bytes: sse_max_line_bytes.max(1),
            sse_max_frame_bytes: sse_max_frame_bytes.max(1),
        }
    }

    /// Same semantics as [`crate::HttpResponse::is_success`].
    ///
    /// # Returns
    /// `true` when status is 2xx.
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    /// Destructures `self`, yielding the body stream for manual polling.
    ///
    /// # Returns
    /// The inner [`HttpByteStream`].
    pub fn into_stream(self) -> HttpByteStream {
        self.stream
    }

    /// Decodes current stream body as SSE events (`UTF-8 lines -> SSE frames`).
    ///
    /// # Returns
    /// Stream yielding parsed SSE events.
    ///
    /// # Errors
    /// Each emitted item may contain:
    /// - transport/read errors forwarded from the underlying HTTP stream;
    /// - [`crate::HttpError::sse_protocol`] when SSE line UTF-8 decoding fails.
    pub fn decode_events(self) -> SseEventStream {
        let max_line_bytes = self.sse_max_line_bytes;
        let max_frame_bytes = self.sse_max_frame_bytes;
        self.decode_events_with_limits(max_line_bytes, max_frame_bytes)
    }

    /// Decodes current stream body as SSE events with explicit line/frame size limits.
    ///
    /// # Parameters
    /// - `max_line_bytes`: Maximum allowed bytes for one SSE line.
    /// - `max_frame_bytes`: Maximum allowed bytes for one SSE frame.
    ///
    /// # Returns
    /// Stream yielding parsed SSE events.
    ///
    /// # Errors
    /// Each emitted item may contain transport/read/protocol errors and limit violations.
    pub fn decode_events_with_limits(
        self,
        max_line_bytes: usize,
        max_frame_bytes: usize,
    ) -> SseEventStream {
        crate::sse::decode_events_from_response_with_limits(self, max_line_bytes, max_frame_bytes)
    }

    /// Decodes SSE `data:` payloads as JSON chunks with response defaults.
    ///
    /// # Parameters
    /// - `done_policy`: Done marker policy (for example `[DONE]`).
    ///
    /// # Type parameters
    /// - `T`: Target chunk type deserialized from each `data:` payload.
    ///
    /// # Returns
    /// Stream yielding [`crate::sse::SseChunk::Data`] and optional
    /// [`crate::sse::SseChunk::Done`].
    ///
    /// # Errors
    /// The stream may emit transport/protocol errors. Malformed JSON behavior is controlled by
    /// the response default JSON mode (configured by [`crate::HttpClientOptions::sse_json_mode`]).
    pub fn decode_json_chunks<T>(self, done_policy: DoneMarkerPolicy) -> SseChunkStream<T>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let mode = self.sse_json_mode;
        let max_line_bytes = self.sse_max_line_bytes;
        let max_frame_bytes = self.sse_max_frame_bytes;
        self.decode_json_chunks_with_mode_and_limits(
            done_policy,
            mode,
            max_line_bytes,
            max_frame_bytes,
        )
    }

    /// Decodes SSE `data:` payloads as JSON chunks with configurable strictness.
    ///
    /// # Parameters
    /// - `done_policy`: Done marker policy (for example `[DONE]`).
    /// - `mode`: JSON decoding strictness for malformed payloads.
    ///
    /// # Type parameters
    /// - `T`: Target chunk type deserialized from each `data:` payload.
    ///
    /// # Returns
    /// Stream yielding [`crate::sse::SseChunk::Data`] and optional
    /// [`crate::sse::SseChunk::Done`].
    ///
    /// # Errors
    /// - transport/read errors from underlying stream;
    /// - protocol errors from SSE parsing;
    /// - in strict mode, [`crate::HttpError::sse_decode`] on malformed JSON payload.
    pub fn decode_json_chunks_with_mode<T>(
        self,
        done_policy: DoneMarkerPolicy,
        mode: SseJsonMode,
    ) -> SseChunkStream<T>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let max_line_bytes = self.sse_max_line_bytes;
        let max_frame_bytes = self.sse_max_frame_bytes;
        self.decode_json_chunks_with_mode_and_limits(
            done_policy,
            mode,
            max_line_bytes,
            max_frame_bytes,
        )
    }

    /// Decodes SSE `data:` payloads as JSON chunks with configurable strictness and limits.
    ///
    /// # Parameters
    /// - `done_policy`: Done marker policy (for example `[DONE]`).
    /// - `mode`: JSON decoding strictness for malformed payloads.
    /// - `max_line_bytes`: Maximum allowed bytes for one SSE line.
    /// - `max_frame_bytes`: Maximum allowed bytes for one SSE frame.
    ///
    /// # Type parameters
    /// - `T`: Target chunk type deserialized from each `data:` payload.
    ///
    /// # Returns
    /// Stream yielding [`crate::sse::SseChunk::Data`] and optional
    /// [`crate::sse::SseChunk::Done`].
    pub fn decode_json_chunks_with_mode_and_limits<T>(
        self,
        done_policy: DoneMarkerPolicy,
        mode: SseJsonMode,
        max_line_bytes: usize,
        max_frame_bytes: usize,
    ) -> SseChunkStream<T>
    where
        T: DeserializeOwned + Send + 'static,
    {
        crate::sse::decode_json_chunks_from_response_with_limits(
            self,
            done_policy,
            mode,
            max_line_bytes,
            max_frame_bytes,
        )
    }
}

impl std::fmt::Debug for HttpStreamResponse {
    /// Debug output includes status, headers, and URL; omits the stream body.
    ///
    /// # Parameters
    /// - `f`: Formatter.
    ///
    /// # Returns
    /// [`std::fmt::Result`].
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpStreamResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("url", &self.url)
            .field("sse_json_mode", &self.sse_json_mode)
            .field("sse_max_line_bytes", &self.sse_max_line_bytes)
            .field("sse_max_frame_bytes", &self.sse_max_frame_bytes)
            .finish_non_exhaustive()
    }
}
