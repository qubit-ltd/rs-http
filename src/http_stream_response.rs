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
            .finish_non_exhaustive()
    }
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
        Self {
            status,
            headers,
            url,
            stream,
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
        crate::sse::decode_events_from_response(self)
    }

    /// Decodes SSE `data:` payloads as JSON chunks in lenient mode.
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
    /// The stream may emit transport/protocol errors; malformed JSON payloads are skipped.
    pub fn decode_json_chunks<T>(self, done_policy: DoneMarkerPolicy) -> SseChunkStream<T>
    where
        T: DeserializeOwned + Send + 'static,
    {
        self.decode_json_chunks_with_mode(done_policy, SseJsonMode::Lenient)
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
        crate::sse::decode_json_chunks_from_response(self, done_policy, mode)
    }
}
