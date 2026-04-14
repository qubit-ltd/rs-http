/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Streaming HTTP response payload and helper methods.

use http::{HeaderMap, StatusCode};
use serde::de::DeserializeOwned;
use url::Url;

use crate::{
    sse::{DoneMarkerPolicy, SseChunkStream, SseEventStream, SseJsonMode},
    HttpByteStream, HttpResponse, HttpResponseMeta, SseDecodeOptions,
};

use super::StreamingBody;

/// Streaming HTTP response alias backed by [`HttpResponse`] and [`StreamingBody`].
pub type StreamingHttpResponse = HttpResponse<StreamingBody>;

impl HttpResponse<StreamingBody> {
    /// Wraps status, headers, URL, and the byte stream.
    ///
    /// # Parameters
    /// - `status`: HTTP status.
    /// - `headers`: Header map.
    /// - `url`: Final URL.
    /// - `stream`: Pinned body stream.
    ///
    /// # Returns
    /// New [`StreamingHttpResponse`].
    pub fn new_stream(
        status: StatusCode,
        headers: HeaderMap,
        url: Url,
        stream: HttpByteStream,
    ) -> Self {
        Self::new_with_sse_decode_options(status, headers, url, stream, SseDecodeOptions::default())
    }

    /// Wraps status, headers, URL, and byte stream with SSE decode options.
    ///
    /// # Parameters
    /// - `status`: HTTP status.
    /// - `headers`: Header map.
    /// - `url`: Final URL.
    /// - `stream`: Pinned body stream.
    /// - `sse_decode_options`: Default options used by SSE decode helpers.
    ///
    /// # Returns
    /// New [`StreamingHttpResponse`].
    pub(crate) fn new_with_sse_decode_options(
        status: StatusCode,
        headers: HeaderMap,
        url: Url,
        stream: HttpByteStream,
        sse_decode_options: SseDecodeOptions,
    ) -> Self {
        let body = StreamingBody {
            stream,
            sse_decode_options: SseDecodeOptions::new(
                sse_decode_options.json_mode,
                sse_decode_options.max_line_bytes,
                sse_decode_options.max_frame_bytes,
            ),
        };
        Self {
            meta: HttpResponseMeta::new(status, headers, url),
            body,
        }
    }

    /// Destructures `self`, yielding the body stream for manual polling.
    ///
    /// # Returns
    /// The inner [`HttpByteStream`].
    pub fn into_stream(self) -> HttpByteStream {
        self.body.stream
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
        let max_line_bytes = self.body.sse_decode_options.max_line_bytes;
        let max_frame_bytes = self.body.sse_decode_options.max_frame_bytes;
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
        let options = self.body.sse_decode_options;
        self.decode_json_chunks_with_mode_and_limits(
            done_policy,
            options.json_mode,
            options.max_line_bytes,
            options.max_frame_bytes,
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
        let options = self.body.sse_decode_options;
        self.decode_json_chunks_with_mode_and_limits(
            done_policy,
            mode,
            options.max_line_bytes,
            options.max_frame_bytes,
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
