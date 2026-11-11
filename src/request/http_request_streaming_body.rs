// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Deferred streaming request body wrapper.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::http_request_body_byte_stream::HttpRequestBodyByteStream;
use crate::HttpResult;

/// Future that produces one request-body byte stream.
type HttpRequestBodyStreamFactoryFuture =
    Pin<Box<dyn Future<Output = HttpResult<HttpRequestBodyByteStream>> + Send + 'static>>;

/// Factory callback that produces a fresh stream for each request attempt.
type HttpRequestBodyStreamFactoryFn = dyn Fn() -> HttpRequestBodyStreamFactoryFuture + Send + Sync + 'static;

/// Deferred streaming upload body source.
///
/// Each send attempt obtains a fresh byte stream from the stored async factory,
/// so retries can rebuild the outbound body stream deterministically.
/// The factory is called once per outer send attempt; it does not create a new
/// body for an internal HTTP redirect. In particular, reqwest cannot replay
/// this streaming body through a `307` or `308` redirect, so the response is
/// returned as a status error unless an outer retry policy retries it. Prefer
/// the final upload URL, or use a buffered body when the payload fits the
/// application's memory budget.
///
/// # Examples
///
/// ```
/// use bytes::Bytes;
/// use futures_util::stream;
/// use qubit_http::HttpRequestBodyByteStream;
/// use qubit_http::HttpRequestStreamingBody;
///
/// let body = HttpRequestStreamingBody::new(|| {
///     Box::pin(async {
///         Box::pin(stream::iter(vec![Ok(Bytes::from_static(b"payload"))]))
///             as HttpRequestBodyByteStream
///     })
/// });
/// assert!(format!("{body:?}").contains("HttpRequestStreamingBody"));
/// ```
#[derive(Clone)]
#[must_use]
pub struct HttpRequestStreamingBody {
    /// Async factory producing one stream for one request attempt.
    factory: Arc<HttpRequestBodyStreamFactoryFn>,
}

impl std::fmt::Debug for HttpRequestStreamingBody {
    /// Formats this type without exposing closure internals.
    ///
    /// # Parameters
    /// - `f`: Formatter destination.
    ///
    /// # Returns
    /// Formatting result.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpRequestStreamingBody").finish_non_exhaustive()
    }
}

impl HttpRequestStreamingBody {
    /// Creates a deferred streaming body from an async stream factory.
    ///
    /// # Parameters
    /// - `factory`: Async factory that returns a fresh byte stream for one send
    ///   attempt.
    ///
    /// # Returns
    /// New deferred streaming body wrapper.
    ///
    /// # Type Parameters
    /// - `F`: Thread-safe factory that creates a fresh stream for one attempt.
    pub fn new<F>(factory: F) -> Self
    where
        F: Fn() -> Pin<Box<dyn Future<Output = HttpRequestBodyByteStream> + Send + 'static>> + Send + Sync + 'static,
    {
        Self::try_new(move || {
            let future = factory();
            Box::pin(async move { Ok(future.await) })
        })
    }

    /// Creates a deferred streaming body from a fallible async factory.
    ///
    /// The factory future runs only when a request attempt is sent. This
    /// constructor does not run or validate it. Preparation errors are
    /// returned directly with their original kind and source. Existing
    /// [`Self::new`] callers do not need to migrate.
    ///
    /// # Parameters
    /// - `factory`: Async factory that prepares a fresh byte stream for one
    ///   send attempt.
    ///
    /// # Returns
    /// New deferred streaming body wrapper.
    ///
    /// # Type Parameters
    /// - `F`: Thread-safe factory that creates a future returning a fresh
    ///   stream or an [`crate::HttpError`].
    ///
    /// # Examples
    ///
    /// ```
    /// use qubit_http::HttpError;
    /// use qubit_http::HttpRequestBodyByteStream;
    /// use qubit_http::HttpRequestStreamingBody;
    ///
    /// async fn prepare_upload() -> Result<HttpRequestBodyByteStream, HttpError> {
    ///     Ok(Box::pin(futures_util::stream::empty()))
    /// }
    ///
    /// let body = HttpRequestStreamingBody::try_new(|| Box::pin(async {
    ///     let stream = prepare_upload().await?;
    ///     Ok(stream)
    /// }));
    /// let failing_body = HttpRequestStreamingBody::try_new(|| Box::pin(async {
    ///     Err(HttpError::other("upload preparation failed"))
    /// }));
    /// assert!(format!("{body:?}").contains("HttpRequestStreamingBody"));
    /// assert!(format!("{failing_body:?}").contains("HttpRequestStreamingBody"));
    /// ```
    pub fn try_new<F>(factory: F) -> Self
    where
        F: Fn() -> HttpRequestBodyStreamFactoryFuture + Send + Sync + 'static,
    {
        Self {
            factory: Arc::new(factory),
        }
    }

    /// Builds reqwest body from one factory-produced byte stream.
    ///
    /// # Returns
    /// New [`reqwest::Body`] stream instance for one send attempt.
    pub(crate) async fn to_reqwest_body(&self) -> HttpResult<reqwest::Body> {
        let stream = (self.factory)().await?;
        Ok(reqwest::Body::wrap_stream(stream))
    }
}
