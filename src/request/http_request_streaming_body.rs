/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Deferred streaming request body wrapper.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::http_request_body_byte_stream::HttpRequestBodyByteStream;

type HttpRequestBodyStreamFactoryFuture =
    Pin<Box<dyn Future<Output = HttpRequestBodyByteStream> + Send + 'static>>;

type HttpRequestBodyStreamFactoryFn =
    dyn Fn() -> HttpRequestBodyStreamFactoryFuture + Send + Sync + 'static;

/// Deferred streaming upload body source.
///
/// Each send attempt obtains a fresh byte stream from the stored async factory,
/// so retries can rebuild the outbound body stream deterministically.
#[derive(Clone)]
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
        f.debug_struct("HttpRequestStreamingBody")
            .finish_non_exhaustive()
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
    pub fn new<F>(factory: F) -> Self
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
    pub(crate) async fn to_reqwest_body(&self) -> reqwest::Body {
        let stream = (self.factory)().await;
        reqwest::Body::wrap_stream(stream)
    }
}
