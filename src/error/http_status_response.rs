// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error;
use std::sync::Arc;

use bytes::Bytes;
use http::HeaderMap;

use super::HttpBodyCaptureEnd;

/// Bounded raw response data retained for a non-success HTTP status.
///
/// The response snapshot is attached to an [`HttpError`](crate::HttpError)
/// when the response body is captured. Its body contains only the retained
/// prefix. [`is_truncated`](Self::is_truncated) is true when source bytes were
/// omitted or capture ended before completeness could be established.
///
/// # Examples
///
/// ```
/// use qubit_http::HttpError;
/// use qubit_http::HttpStatusResponse;
///
/// fn inspect(error: &HttpError) {
///     if let Some(response) = error.status_response() {
///         let _reason = response.capture_end();
///         if let Some(source) = response.capture_error() {
///             // Redact source details before logging.
///             let _source_to_redact = source;
///         }
///     }
/// }
/// let _inspect: fn(&HttpError) = inspect;
/// ```
#[derive(Clone)]
pub struct HttpStatusResponse {
    /// Headers received with the non-success response.
    headers: HeaderMap,
    /// Retained prefix of the response body.
    body: Bytes,
    /// Final reason status-body capture ended.
    capture_end: HttpBodyCaptureEnd,
    /// Backend body-read failure, retained for explicit diagnostics.
    capture_error: Option<Arc<dyn Error + Send + Sync + 'static>>,
    /// Complete body length advertised by the server, if known.
    content_length: Option<u64>,
}

impl HttpStatusResponse {
    /// Creates a bounded snapshot of a non-success response.
    ///
    /// # Parameters
    /// - `headers`: Headers received with the response.
    /// - `body`: Retained prefix of the response body.
    /// - `capture_end`: Final reason status-body capture ended.
    /// - `capture_error`: Backend body-read error, if capture failed.
    /// - `content_length`: Complete body length advertised by the server, if
    ///   known.
    ///
    /// # Returns
    /// A response snapshot containing the supplied headers and body metadata.
    pub(crate) fn new(
        headers: HeaderMap,
        body: Bytes,
        capture_end: HttpBodyCaptureEnd,
        capture_error: Option<Arc<dyn Error + Send + Sync + 'static>>,
        content_length: Option<u64>,
    ) -> Self {
        debug_assert!(capture_error.is_none() || capture_end == HttpBodyCaptureEnd::ReadFailure);
        Self {
            headers,
            body,
            capture_end,
            capture_error,
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
    /// Reports whether the body capture is incomplete or known to omit bytes.
    ///
    /// This flag does not identify the cause or prove that additional bytes
    /// were available.
    ///
    /// # Returns
    /// `true` when the size limit omitted bytes, or when a read timeout or
    /// transport failure prevented complete capture. `false` means capture
    /// completed without omitting source bytes.
    #[must_use]
    #[inline]
    pub fn is_truncated(&self) -> bool {
        self.capture_end != HttpBodyCaptureEnd::Complete
    }
    /// Returns the reason response-body capture ended.
    #[must_use]
    #[inline]
    pub fn capture_end(&self) -> HttpBodyCaptureEnd {
        self.capture_end
    }

    /// Returns the backend read error for [`HttpBodyCaptureEnd::ReadFailure`].
    ///
    /// This exposes the original error and its source chain only for
    /// [`HttpBodyCaptureEnd::ReadFailure`]. Callers must redact sensitive
    /// details before logging it.
    #[must_use]
    pub fn capture_error(&self) -> Option<&(dyn Error + Send + Sync + 'static)> {
        self.capture_error.as_deref()
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
            .field("capture_end", &self.capture_end)
            .field("capture_error_present", &self.capture_error.is_some())
            .field("content_length", &self.content_length)
            .finish()
    }
}
