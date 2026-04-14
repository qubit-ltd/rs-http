/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Buffered HTTP response specialization and helpers.

use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use serde::de::DeserializeOwned;
use url::Url;

use crate::{HttpError, HttpResult};

use super::{HttpResponse, HttpResponseMeta};

/// Buffered HTTP response type alias.
pub type BufferedHttpResponse = HttpResponse<Bytes>;

impl HttpResponse<Bytes> {
    /// Assembles a buffered response from status/headers/body/url parts.
    ///
    /// # Parameters
    /// - `status`: HTTP status line code.
    /// - `headers`: Response headers.
    /// - `body`: Full body bytes.
    /// - `url`: Final URL after redirects.
    ///
    /// # Returns
    /// New [`BufferedHttpResponse`].
    #[inline]
    pub fn new(status: StatusCode, headers: HeaderMap, body: Bytes, url: Url) -> Self {
        Self::new_with_meta(HttpResponseMeta::new(status, headers, url), body)
    }

    /// Interprets buffered body as UTF-8 text.
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

    /// Deserializes buffered body as JSON into `T`.
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
