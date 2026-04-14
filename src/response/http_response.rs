/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Buffered HTTP response type and methods.

use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use url::Url;

use super::HttpResponseMeta;

/// HTTP response metadata plus body payload.
#[derive(Debug, Clone)]
pub struct HttpResponse<B = Bytes> {
    /// Response metadata (status, headers, final URL).
    pub meta: HttpResponseMeta,
    /// Response body payload.
    pub body: B,
}

impl<B> HttpResponse<B> {
    /// Assembles a response from metadata and body payload.
    ///
    /// # Parameters
    /// - `meta`: Response metadata.
    /// - `body`: Response body payload.
    ///
    /// # Returns
    /// New [`HttpResponse`].
    #[inline]
    pub fn new_with_meta(meta: HttpResponseMeta, body: B) -> Self {
        Self {
            meta,
            body,
        }
    }

    /// Returns shared response metadata.
    ///
    /// # Returns
    /// Immutable metadata reference.
    #[inline]
    pub fn meta(&self) -> &HttpResponseMeta {
        &self.meta
    }

    /// Returns response status code.
    ///
    /// # Returns
    /// HTTP status code from [`HttpResponse::meta`].
    #[inline]
    pub fn status(&self) -> StatusCode {
        self.meta.status
    }

    /// Returns response headers.
    ///
    /// # Returns
    /// Immutable headers from [`HttpResponse::meta`].
    #[inline]
    pub fn headers(&self) -> &HeaderMap {
        &self.meta.headers
    }

    /// Returns final response URL.
    ///
    /// # Returns
    /// Final URL from [`HttpResponse::meta`].
    #[inline]
    pub fn url(&self) -> &Url {
        &self.meta.url
    }

    /// Returns whether [`HttpResponse::status`] is a success code ([`StatusCode::is_success`]).
    ///
    /// # Returns
    /// `true` for 2xx responses.
    #[inline]
    pub fn is_success(&self) -> bool {
        self.status().is_success()
    }
}

