/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # HTTP Client
//!
//! Implements request execution and stream execution with unified behavior.
//!
//! # Author
//!
//! Haixing Hu

use std::sync::{Arc, RwLock};

use async_stream::stream;
use bytes::Bytes;
use futures_util::StreamExt;
use http::HeaderMap;
use reqwest::Response;
use url::Url;

use crate::logging::{log_request, log_response, log_stream_response_headers};
use crate::{
    HeaderInjector, HttpError, HttpErrorKind, HttpRequest, HttpRequestBody, HttpRequestBuilder,
    HttpResponse, HttpResult, HttpStreamResponse,
};

/// Shared state for [`HttpClient`]: underlying reqwest client, options, and header injectors.
#[derive(Clone)]
struct Inner {
    /// Low-level HTTP client used to send requests.
    client: reqwest::Client,
    /// Timeouts, proxy, logging, default headers, and related settings.
    options: crate::HttpClientOptions,
    /// Dynamic header injectors applied to every outgoing request after default headers.
    injectors: Arc<RwLock<Vec<Arc<dyn HeaderInjector>>>>,
}

/// High-level HTTP client that applies options, header injection, logging, and timeouts.
#[derive(Clone)]
pub struct HttpClient {
    /// Shared inner state (thread-safe via [`Arc`]).
    inner: Arc<Inner>,
}

impl std::fmt::Debug for HttpClient {
    /// Formats the client for debugging (exposes options only; omits injectors and reqwest client).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field("options", &self.inner.options)
            .finish_non_exhaustive()
    }
}

impl HttpClient {
    /// Wraps a built [`reqwest::Client`] with the given options and an empty injector list.
    ///
    /// # Parameters
    /// - `client`: Configured reqwest client used for I/O.
    /// - `options`: Client-wide timeouts, headers, proxy, logging, etc.
    ///
    /// # Returns
    /// A new [`HttpClient`] sharing no injectors until [`HttpClient::add_header_injector`] is called.
    pub(crate) fn new(client: reqwest::Client, options: crate::HttpClientOptions) -> Self {
        Self {
            inner: Arc::new(Inner {
                client,
                options,
                injectors: Arc::new(RwLock::new(Vec::new())),
            }),
        }
    }

    /// Returns a reference to the client options (timeouts, proxy, logging, etc.).
    ///
    /// # Returns
    /// Immutable borrow of [`crate::HttpClientOptions`].
    pub fn options(&self) -> &crate::HttpClientOptions {
        &self.inner.options
    }

    /// Appends a [`HeaderInjector`] so its [`HeaderInjector::inject`] runs on every request.
    ///
    /// # Parameters
    /// - `injector`: Shared injector to append (order is preserved).
    pub fn add_header_injector(&self, injector: Arc<dyn HeaderInjector>) {
        if let Ok(mut guard) = self.inner.injectors.write() {
            guard.push(injector);
        }
    }

    /// Removes all registered header injectors.
    pub fn clear_header_injectors(&self) {
        if let Ok(mut guard) = self.inner.injectors.write() {
            guard.clear();
        }
    }

    /// Starts building an [`HttpRequest`] with the given method and path (relative or absolute URL string).
    ///
    /// # Parameters
    /// - `method`: HTTP verb (GET, POST, …).
    /// - `path`: Path relative to [`crate::HttpClientOptions::base_url`] or a full URL string.
    ///
    /// # Returns
    /// A fresh [`HttpRequestBuilder`] not yet tied to this client until [`HttpRequestBuilder::build`] and [`HttpClient::execute`].
    pub fn request(&self, method: http::Method, path: impl AsRef<str>) -> HttpRequestBuilder {
        HttpRequestBuilder::new(method, path)
    }

    /// Sends the request, reads the full response body, logs per options, and returns a buffered [`HttpResponse`].
    ///
    /// # Parameters
    /// - `request`: Built request (URL resolved against `base_url` if path is not absolute).
    ///
    /// # Returns
    /// - `Ok(HttpResponse)` when the HTTP status is success ([`http::StatusCode::is_success`]).
    /// - `Err(HttpError)` on URL/header errors, transport failure, timeout, or non-success status.
    pub async fn execute(&self, request: HttpRequest) -> HttpResult<HttpResponse> {
        let url = self.resolve_url(&request)?;
        let method = request.method.clone();
        let headers = self.build_headers(&request)?;

        let body_for_log = match &request.body {
            HttpRequestBody::Bytes(bytes) | HttpRequestBody::Json(bytes) => Some(bytes.clone()),
            HttpRequestBody::Text(text) => Some(Bytes::from(text.clone())),
            HttpRequestBody::Empty => None,
        };

        log_request(
            &method,
            &url,
            &headers,
            body_for_log.as_ref(),
            &self.inner.options.logging,
            &self.inner.options.sensitive_headers,
        );

        let mut builder = self.inner.client.request(method.clone(), url.clone());
        builder = builder.headers(headers);

        if !request.query.is_empty() {
            builder = builder.query(&request.query);
        }

        if let Some(timeout) = request.request_timeout {
            builder = builder.timeout(timeout);
        }

        builder = match request.body {
            HttpRequestBody::Empty => builder,
            HttpRequestBody::Bytes(bytes) => builder.body(bytes),
            HttpRequestBody::Text(text) => builder.body(text),
            HttpRequestBody::Json(bytes) => builder.body(bytes),
        };

        let response = self
            .send_with_write_timeout(builder, method.clone(), url.clone())
            .await?;

        if !response.status().is_success() {
            return Err(HttpError::status(
                response.status(),
                format!(
                    "HTTP request failed with status {} for {} {}",
                    response.status(),
                    method,
                    url
                ),
            )
            .with_method(method)
            .with_url(url));
        }

        let status = response.status();
        let response_url = response.url().clone();
        let response_headers = response.headers().clone();

        let body = self
            .read_body_with_timeout(response, method.clone(), response_url.clone())
            .await?;

        log_response(
            status,
            &response_url,
            &response_headers,
            &body,
            &self.inner.options.logging,
            &self.inner.options.sensitive_headers,
        );

        Ok(HttpResponse::new(
            status,
            response_headers,
            body,
            response_url,
        ))
    }

    /// Sends the request and returns headers plus a byte stream without buffering the full body.
    ///
    /// # Parameters
    /// - `request`: Same as [`HttpClient::execute`].
    ///
    /// # Returns
    /// - `Ok(HttpStreamResponse)` with a stream that applies read timeout per options.
    /// - `Err(HttpError)` on failure before streaming starts (same cases as execute for the initial response).
    pub async fn execute_stream(&self, request: HttpRequest) -> HttpResult<HttpStreamResponse> {
        let url = self.resolve_url(&request)?;
        let method = request.method.clone();
        let headers = self.build_headers(&request)?;

        let body_for_log = match &request.body {
            HttpRequestBody::Bytes(bytes) | HttpRequestBody::Json(bytes) => Some(bytes.clone()),
            HttpRequestBody::Text(text) => Some(Bytes::from(text.clone())),
            HttpRequestBody::Empty => None,
        };

        log_request(
            &method,
            &url,
            &headers,
            body_for_log.as_ref(),
            &self.inner.options.logging,
            &self.inner.options.sensitive_headers,
        );

        let mut builder = self.inner.client.request(method.clone(), url.clone());
        builder = builder.headers(headers);

        if !request.query.is_empty() {
            builder = builder.query(&request.query);
        }

        if let Some(timeout) = request.request_timeout {
            builder = builder.timeout(timeout);
        }

        builder = match request.body {
            HttpRequestBody::Empty => builder,
            HttpRequestBody::Bytes(bytes) => builder.body(bytes),
            HttpRequestBody::Text(text) => builder.body(text),
            HttpRequestBody::Json(bytes) => builder.body(bytes),
        };

        let response = self
            .send_with_write_timeout(builder, method.clone(), url.clone())
            .await?;

        if !response.status().is_success() {
            return Err(HttpError::status(
                response.status(),
                format!(
                    "HTTP streaming request failed with status {} for {} {}",
                    response.status(),
                    method,
                    url
                ),
            )
            .with_method(method)
            .with_url(url));
        }

        let status = response.status();
        let response_url = response.url().clone();
        let response_headers = response.headers().clone();

        log_stream_response_headers(
            status,
            &response_url,
            &response_headers,
            &self.inner.options.logging,
            &self.inner.options.sensitive_headers,
        );

        let read_timeout = self.inner.options.timeouts.read_timeout;
        let method_for_err = method.clone();
        let url_for_err = response_url.clone();

        let mut stream = response.bytes_stream();
        let wrapped = stream! {
            loop {
                let next = tokio::time::timeout(read_timeout, stream.next()).await;
                match next {
                    Ok(Some(Ok(bytes))) => yield Ok(bytes),
                    Ok(Some(Err(error))) => {
                        let mapped = map_reqwest_error(
                            error,
                            HttpErrorKind::Transport,
                            Some(method_for_err.clone()),
                            Some(url_for_err.clone()),
                        );
                        yield Err(mapped);
                        break;
                    }
                    Ok(None) => break,
                    Err(_) => {
                        let error = HttpError::read_timeout(format!(
                            "Read timeout after {:?} while streaming response",
                            read_timeout
                        ))
                        .with_method(method_for_err.clone())
                        .with_url(url_for_err.clone());
                        yield Err(error);
                        break;
                    }
                }
            }
        };

        Ok(HttpStreamResponse::new(
            status,
            response_headers,
            response_url,
            Box::pin(wrapped),
        ))
    }

    /// Parses `request.path` as a URL or joins it to `base_url` when relative.
    ///
    /// # Parameters
    /// - `request`: Request whose `path` and implied base are used.
    ///
    /// # Returns
    /// Resolved [`Url`] or [`HttpError::invalid_url`] if resolution fails.
    fn resolve_url(&self, request: &HttpRequest) -> HttpResult<Url> {
        if let Ok(url) = Url::parse(&request.path) {
            return Ok(url);
        }

        let base = self.inner.options.base_url.as_ref().ok_or_else(|| {
            HttpError::invalid_url(format!(
                "Cannot resolve relative path '{}' without base_url",
                request.path
            ))
        })?;

        base.join(&request.path).map_err(|error| {
            HttpError::invalid_url(format!(
                "Failed to resolve path '{}' against base URL '{}': {}",
                request.path, base, error
            ))
        })
    }

    /// Merges default headers, injector output, and per-request headers (later wins on duplicates).
    ///
    /// # Parameters
    /// - `request`: Request supplying extra headers.
    ///
    /// # Returns
    /// Final [`HeaderMap`] or error if an injector fails.
    fn build_headers(&self, request: &HttpRequest) -> HttpResult<HeaderMap> {
        let mut headers = self.inner.options.default_headers.clone();

        if let Ok(guard) = self.inner.injectors.read() {
            for injector in guard.iter() {
                injector.inject(&mut headers)?;
            }
        }

        headers.extend(request.headers.clone());
        Ok(headers)
    }

    /// Sends the built request with a write-phase timeout (time to finish sending the request).
    ///
    /// # Parameters
    /// - `builder`: Reqwest request builder (method, URL, headers, body already set).
    /// - `method`: Method for error context.
    /// - `url`: URL for error context.
    ///
    /// # Returns
    /// Raw [`reqwest::Response`] or [`HttpError`] (transport, write timeout, etc.).
    async fn send_with_write_timeout(
        &self,
        builder: reqwest::RequestBuilder,
        method: http::Method,
        url: Url,
    ) -> HttpResult<Response> {
        let timeout = self.inner.options.timeouts.write_timeout;
        match tokio::time::timeout(timeout, builder.send()).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(error)) => Err(map_reqwest_error(
                error,
                HttpErrorKind::Transport,
                Some(method),
                Some(url),
            )),
            Err(_) => Err(HttpError::write_timeout(format!(
                "Write timeout after {:?} while sending request",
                timeout
            ))
            .with_method(method)
            .with_url(url)),
        }
    }

    /// Reads the entire response body with a read timeout.
    ///
    /// # Parameters
    /// - `response`: Successful response whose body will be consumed.
    /// - `method`: Method for error context.
    /// - `url`: URL for error context.
    ///
    /// # Returns
    /// Body as [`Bytes`] or [`HttpError`] (decode/read timeout).
    async fn read_body_with_timeout(
        &self,
        response: Response,
        method: http::Method,
        url: Url,
    ) -> HttpResult<Bytes> {
        let timeout = self.inner.options.timeouts.read_timeout;
        match tokio::time::timeout(timeout, response.bytes()).await {
            Ok(Ok(body)) => Ok(body),
            Ok(Err(error)) => Err(map_reqwest_error(
                error,
                HttpErrorKind::Decode,
                Some(method),
                Some(url),
            )),
            Err(_) => Err(HttpError::read_timeout(format!(
                "Read timeout after {:?} while reading response body",
                timeout
            ))
            .with_method(method)
            .with_url(url)),
        }
    }
}

/// Maps a [`reqwest::Error`] into [`HttpError`] with best-effort [`HttpErrorKind`] and optional context.
///
/// # Parameters
/// - `error`: Underlying reqwest error.
/// - `default_kind`: Kind used when reqwest does not classify the error more specifically.
/// - `method`: Optional request method to attach.
/// - `url`: Optional request URL to attach.
///
/// # Returns
/// Configured [`HttpError`] including chained source.
fn map_reqwest_error(
    error: reqwest::Error,
    default_kind: HttpErrorKind,
    method: Option<http::Method>,
    url: Option<Url>,
) -> HttpError {
    let kind = if error.is_timeout() {
        HttpErrorKind::ConnectTimeout
    } else if error.is_request() {
        HttpErrorKind::InvalidUrl
    } else if error.is_decode() {
        HttpErrorKind::Decode
    } else if error.is_status() {
        HttpErrorKind::Status
    } else {
        default_kind
    };

    let mut result = HttpError::new(kind, format!("HTTP transport error: {}", error));
    if let Some(method) = method {
        result = result.with_method(method);
    }
    if let Some(url) = url {
        result = result.with_url(url);
    }
    result.with_source(error)
}
