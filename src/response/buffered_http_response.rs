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
use http::{HeaderMap, Method, StatusCode};
use serde::de::DeserializeOwned;
use url::Url;

use crate::client::error_mapper::{map_reqwest_error, ReqwestErrorPhase};
use crate::{HttpError, HttpErrorKind, HttpRequest, HttpResult};

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
    /// - `method`: Originating request method.
    ///
    /// # Returns
    /// New [`BufferedHttpResponse`].
    #[inline]
    pub fn new(
        status: StatusCode,
        headers: HeaderMap,
        body: Bytes,
        url: Url,
        method: Method,
    ) -> Self {
        Self::new_with_meta(HttpResponseMeta::new(status, headers, url, method), body)
    }

    /// Builds a buffered response from reqwest response and request read context.
    ///
    /// # Parameters
    /// - `response`: Raw reqwest response; body will be consumed.
    /// - `request`: Request context carrying method/read-timeout/cancellation settings.
    /// - `context_url`: URL used in mapped read/cancellation errors.
    ///
    /// # Returns
    /// New [`BufferedHttpResponse`] with status/headers/url copied from `response`.
    ///
    /// # Errors
    /// Returns mapped decode/read-timeout/cancellation errors while reading body bytes.
    pub(crate) async fn try_new(
        response: reqwest::Response,
        request: &HttpRequest,
        context_url: Url,
    ) -> HttpResult<Self> {
        let method = request.method().clone();
        let meta = HttpResponseMeta::new(
            response.status(),
            response.headers().clone(),
            response.url().clone(),
            method.clone(),
        );
        let timeout = request.read_timeout();
        let read_future = tokio::time::timeout(timeout, response.bytes());
        let next = if let Some(token) = request.cancellation_token() {
            tokio::select! {
                _ = token.cancelled() => {
                    return Err(HttpError::cancelled("Request cancelled while reading response body")
                        .with_method(&method)
                        .with_url(&context_url));
                }
                read_result = read_future => read_result,
            }
        } else {
            read_future.await
        };
        match next {
            Ok(Ok(body)) => Ok(Self::new_with_meta(meta, body)),
            Ok(Err(error)) => Err(map_reqwest_error(
                error,
                HttpErrorKind::Decode,
                Some(ReqwestErrorPhase::Read),
                Some(method.clone()),
                Some(context_url.clone()),
            )),
            Err(_) => Err(HttpError::read_timeout(format!(
                "Read timeout after {:?} while reading response body",
                timeout
            ))
            .with_method(&method)
            .with_url(&context_url)),
        }
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
            .with_url(&self.meta.url)
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
                .with_url(&self.meta.url)
        })
    }
}
