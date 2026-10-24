// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use bytes::Bytes;
use http::HeaderMap;

/// Bounded raw response data retained for a non-success HTTP status.
///
/// The response snapshot is attached to an [`HttpError`](crate::HttpError)
/// when the response body is captured. Its body contains only the retained
/// prefix; [`is_truncated`](Self::is_truncated) indicates whether more data was
/// available.
///
/// # Examples
///
/// ```
/// use qubit_http::HttpError;
/// use qubit_http::HttpStatusResponse;
///
/// let error = HttpError::transport("connection closed");
/// let response: Option<&HttpStatusResponse> = error.status_response();
/// assert!(response.is_none());
/// ```
#[derive(Clone)]
pub struct HttpStatusResponse {
    /// Headers received with the non-success response.
    headers: HeaderMap,
    /// Retained prefix of the response body.
    body: Bytes,
    /// Whether the retained body omits trailing response data.
    truncated: bool,
    /// Complete body length advertised by the server, if known.
    content_length: Option<u64>,
}

impl HttpStatusResponse {
    /// Creates a bounded snapshot of a non-success response.
    ///
    /// # Parameters
    /// - `headers`: Headers received with the response.
    /// - `body`: Retained prefix of the response body.
    /// - `truncated`: Whether the retained body omits trailing response data.
    /// - `content_length`: Complete body length advertised by the server, if
    ///   known.
    ///
    /// # Returns
    /// A response snapshot containing the supplied headers and body metadata.
    pub(crate) fn new(headers: HeaderMap, body: Bytes, truncated: bool, content_length: Option<u64>) -> Self {
        Self {
            headers,
            body,
            truncated,
            content_length,
        }
    }
    /// Returns the headers received with the response.
    ///
    /// The returned map is borrowed from this snapshot and is not copied.
    ///
    /// # Returns
    /// The response headers associated with this snapshot.
    #[must_use]
    #[inline]
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
    /// Returns the retained prefix of the raw response body.
    ///
    /// The returned bytes are borrowed from this snapshot and are not copied.
    ///
    /// # Returns
    /// The retained body prefix.
    #[must_use]
    #[inline]
    pub fn body(&self) -> &Bytes {
        &self.body
    }
    /// Reports whether the retained body omits trailing response data.
    ///
    /// # Returns
    /// `true` when the body prefix is incomplete, otherwise `false`.
    #[must_use]
    #[inline]
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }
    /// Returns the complete body length advertised by the server, if known.
    ///
    /// `None` means that no complete length was available from the response.
    ///
    /// # Returns
    /// The advertised complete body length, or `None` when it is unknown.
    #[inline]
    pub fn content_length(&self) -> Option<u64> {
        self.content_length
    }
}

impl std::fmt::Debug for HttpStatusResponse {
    /// Formats response metadata without exposing headers or body contents.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpStatusResponse")
            .field("body_len", &self.body.len())
            .field("truncated", &self.truncated)
            .field("content_length", &self.content_length)
            .finish()
    }
}
