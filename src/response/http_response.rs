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
use serde::de::DeserializeOwned;
use url::Url;

use crate::{HttpError, HttpResult};

use super::HttpResponseMeta;

/// Complete HTTP response after the body has been read into memory.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// Response metadata (status, headers, final URL).
    pub meta: HttpResponseMeta,
    /// Response body bytes.
    pub body: Bytes,
}

impl HttpResponse {
    /// Assembles a response from its parts (as produced by [`crate::HttpClient::execute`]).
    ///
    /// # Parameters
    /// - `status`: HTTP status line code.
    /// - `headers`: Response headers.
    /// - `body`: Full body bytes.
    /// - `url`: Final URL after redirects.
    ///
    /// # Returns
    /// New [`HttpResponse`].
    pub fn new(status: StatusCode, headers: HeaderMap, body: Bytes, url: Url) -> Self {
        Self::new_with_meta(HttpResponseMeta::new(status, headers, url), body)
    }

    /// Assembles a response from metadata and buffered body bytes.
    ///
    /// # Parameters
    /// - `meta`: Response metadata.
    /// - `body`: Full body bytes.
    ///
    /// # Returns
    /// New [`HttpResponse`].
    pub fn new_with_meta(meta: HttpResponseMeta, body: Bytes) -> Self {
        Self {
            meta,
            body,
        }
    }

    /// Returns shared response metadata.
    ///
    /// # Returns
    /// Immutable metadata reference.
    pub fn meta(&self) -> &HttpResponseMeta {
        &self.meta
    }

    /// Returns response status code.
    ///
    /// # Returns
    /// HTTP status code from [`HttpResponse::meta`].
    pub fn status(&self) -> StatusCode {
        self.meta.status
    }

    /// Returns response headers.
    ///
    /// # Returns
    /// Immutable headers from [`HttpResponse::meta`].
    pub fn headers(&self) -> &HeaderMap {
        &self.meta.headers
    }

    /// Returns final response URL.
    ///
    /// # Returns
    /// Final URL from [`HttpResponse::meta`].
    pub fn url(&self) -> &Url {
        &self.meta.url
    }

    /// Returns whether [`HttpResponse::status`] is a success code ([`StatusCode::is_success`]).
    ///
    /// # Returns
    /// `true` for 2xx responses.
    pub fn is_success(&self) -> bool {
        self.status().is_success()
    }

    /// Interprets [`HttpResponse::body`] as UTF-8 text.
    ///
    /// # Returns
    /// `Ok(String)` or [`HttpError::decode`] with status/URL context on invalid UTF-8.
    pub fn text(&self) -> HttpResult<String> {
        String::from_utf8(self.body.to_vec()).map_err(|error| {
            HttpError::decode(format!(
                "Failed to decode response body as UTF-8: {}",
                error
            ))
            .with_status(self.meta.status)
            .with_url(self.meta.url.clone())
        })
    }

    /// Deserializes [`HttpResponse::body`] as JSON into `T`.
    ///
    /// # Type parameters
    /// - `T`: Type implementing [`serde::de::DeserializeOwned`].
    ///
    /// # Returns
    /// `Ok(T)` or [`HttpError::decode`] with status/URL context.
    pub fn json<T>(&self) -> HttpResult<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_slice(&self.body).map_err(|error| {
            HttpError::decode(format!("Failed to decode response JSON: {}", error))
                .with_status(self.meta.status)
                .with_url(self.meta.url.clone())
        })
    }
}
