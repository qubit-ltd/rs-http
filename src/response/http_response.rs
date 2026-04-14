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

/// Complete HTTP response after the body has been read into memory.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// Response status code.
    pub status: StatusCode,
    /// Response headers.
    pub headers: HeaderMap,
    /// Response body bytes.
    pub body: Bytes,
    /// Final resolved URL.
    pub url: Url,
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
        Self {
            status,
            headers,
            body,
            url,
        }
    }

    /// Returns whether [`HttpResponse::status`] is a success code ([`StatusCode::is_success`]).
    ///
    /// # Returns
    /// `true` for 2xx responses.
    pub fn is_success(&self) -> bool {
        self.status.is_success()
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
            .with_status(self.status)
            .with_url(self.url.clone())
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
                .with_status(self.status)
                .with_url(self.url.clone())
        })
    }
}
