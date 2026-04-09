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
use url::Url;

use crate::HttpByteStream;

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
}
