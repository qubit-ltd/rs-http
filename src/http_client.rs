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

use async_stream::stream;
use bytes::Bytes;
use futures_util::StreamExt;
use http::HeaderMap;
use qubit_function::MutatingFunction;
use qubit_retry::{
    AttemptFailure, Jitter, RetryDecision, RetryError, RetryExecutor, RetryOptions, RetryResult,
};
use reqwest::Response;
use url::Url;

use crate::{
    HeaderInjector, HttpClientOptions, HttpError, HttpErrorKind, HttpLogger, HttpRequest,
    HttpRequestBody, HttpRequestBuilder, HttpResponse, HttpResult, HttpStreamResponse, RetryHint,
};

/// High-level HTTP client that applies options, header injection, logging, and
/// timeouts.
#[derive(Clone)]
pub struct HttpClient {
    /// Low-level HTTP client used to send requests.
    client: reqwest::Client,
    /// Timeouts, proxy, logging, default headers, and related settings.
    options: HttpClientOptions,
    /// Header injectors applied to every outgoing request after default
    /// headers.
    injectors: Vec<HeaderInjector>,
}

impl std::fmt::Debug for HttpClient {
    /// Formats the client for debugging (exposes options and injectors; omits
    /// the reqwest client).
    ///
    /// # Parameters
    /// - `f`: Destination formatter.
    ///
    /// # Returns
    /// `fmt::Result` from writing the debug struct.
    ///
    /// # Errors
    /// Returns an error if formatting to `f` fails.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field("options", &self.options)
            .field("injectors", &self.injectors)
            .finish_non_exhaustive()
    }
}

impl HttpClient {
    /// Wraps a built [`reqwest::Client`] with the given options and an empty
    /// injector list.
    ///
    /// # Parameters
    /// - `client`: Configured reqwest client used for I/O.
    /// - `options`: Client-wide timeouts, headers, proxy, logging, etc.
    ///
    /// # Returns
    /// A new [`HttpClient`] with no injectors until
    /// [`HttpClient::add_header_injector`] is called.
    pub(crate) fn new(client: reqwest::Client, options: HttpClientOptions) -> Self {
        Self {
            client,
            options,
            injectors: Vec::new(),
        }
    }

    /// Returns a reference to the client options (timeouts, proxy, logging,
    /// etc.).
    ///
    /// # Returns
    /// Immutable borrow of [`HttpClientOptions`].
    pub fn options(&self) -> &HttpClientOptions {
        &self.options
    }

    /// Appends a [`HeaderInjector`] so its mutation function runs on every
    /// request.
    ///
    /// # Parameters
    /// - `injector`: Injector to append (order is preserved).
    ///
    /// # Returns
    /// Nothing.
    pub fn add_header_injector(&mut self, injector: HeaderInjector) {
        self.injectors.push(injector);
    }

    /// Validates and adds one client-level default header.
    ///
    /// The header is applied to every request before header injectors and
    /// request-level headers.
    ///
    /// # Parameters
    /// - `name`: Header name.
    /// - `value`: Header value.
    ///
    /// # Returns
    /// `Ok(self)` after the header is stored.
    ///
    /// # Errors
    /// Returns [`HttpError`] when the header name or value is invalid.
    pub fn add_header(&mut self, name: &str, value: &str) -> HttpResult<&mut Self> {
        self.options.add_header(name, value)?;
        Ok(self)
    }

    /// Validates and adds many client-level default headers atomically.
    ///
    /// If any input pair is invalid, no header from this batch is applied.
    ///
    /// # Parameters
    /// - `headers`: Iterator of `(name, value)` pairs.
    ///
    /// # Returns
    /// `Ok(self)` after all headers are stored.
    ///
    /// # Errors
    /// Returns [`HttpError`] when any name/value pair is invalid (nothing from
    /// this call is applied).
    pub fn add_headers<'a, I>(&mut self, headers: I) -> HttpResult<&mut Self>
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        self.options.add_headers(headers)?;
        Ok(self)
    }

    /// Removes all registered header injectors.
    ///
    /// # Returns
    /// Nothing.
    pub fn clear_header_injectors(&mut self) {
        self.injectors.clear();
    }

    /// Starts building an [`HttpRequest`] with the given method and path
    /// (relative or absolute URL string).
    ///
    /// # Parameters
    /// - `method`: HTTP verb (GET, POST, …).
    /// - `path`: Path relative to [`HttpClientOptions::base_url`] or a full URL
    ///   string.
    ///
    /// # Returns
    /// A fresh [`HttpRequestBuilder`] not yet tied to this client until
    /// [`HttpRequestBuilder::build`] and [`HttpClient::execute`].
    pub fn request(&self, method: http::Method, path: &str) -> HttpRequestBuilder {
        HttpRequestBuilder::new(method, path)
    }

    /// Sends the request, reads the full response body, logs per options, and
    /// returns a buffered [`HttpResponse`].
    ///
    /// # Parameters
    /// - `request`: Built request (URL resolved against `base_url` if path is
    ///   not absolute).
    ///
    /// # Returns
    /// - `Ok(HttpResponse)` when the HTTP status is success
    ///   ([`http::StatusCode::is_success`]).
    /// - `Err(HttpError)` on URL/header errors, transport failure, timeout, or
    ///   non-success status.
    pub async fn execute(&self, request: HttpRequest) -> HttpResult<HttpResponse> {
        if !self.should_retry_request(&request) {
            return self.execute_once(request).await;
        }
        self.execute_with_retry(request).await
    }

    /// Performs one non-retrying execution: resolve URL, merge headers, log the
    /// request, send with write timeout, reject non-success status, read the
    /// full body with read timeout, then log the response.
    ///
    /// # Parameters
    /// - `request`: Built request to send (same fields as for
    ///   [`HttpClient::execute`]).
    ///
    /// # Returns
    /// Buffered [`HttpResponse`] or [`HttpError`].
    async fn execute_once(&self, request: HttpRequest) -> HttpResult<HttpResponse> {
        let url = self.resolve_url(&request)?;
        let method = request.method.clone();
        let headers = self.build_headers(&request)?;

        let body_for_log = match &request.body {
            HttpRequestBody::Bytes(bytes) | HttpRequestBody::Json(bytes) => Some(bytes.clone()),
            HttpRequestBody::Text(text) => Some(Bytes::from(text.clone())),
            HttpRequestBody::Empty => None,
        };

        let logger = HttpLogger::new(&self.options.logging, &self.options.sensitive_headers);
        logger.log_request(&method, &url, &headers, body_for_log.as_ref());

        let mut builder = self.client.request(method.clone(), url.clone());
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
            let message = format!(
                "HTTP request failed with status {} for {} {}",
                response.status(),
                method,
                url
            );
            let error = response.error_for_status_ref().expect_err(
                "non-success HTTP status must produce reqwest status error via error_for_status_ref",
            );
            let mut mapped = map_reqwest_error(
                error,
                HttpErrorKind::Status,
                Some(method.clone()),
                Some(url.clone()),
            )
            .with_status(response.status());
            mapped.message = message;
            return Err(mapped);
        }

        let status = response.status();
        let response_url = response.url().clone();
        let response_headers = response.headers().clone();

        let body = self
            .read_body_with_timeout(response, method.clone(), response_url.clone())
            .await?;

        logger.log_response(status, &response_url, &response_headers, &body);

        Ok(HttpResponse::new(
            status,
            response_headers,
            body,
            response_url,
        ))
    }

    /// Sends the request and returns headers plus a byte stream without
    /// buffering the full body.
    ///
    /// # Parameters
    /// - `request`: Same as [`HttpClient::execute`].
    ///
    /// # Returns
    /// - `Ok(HttpStreamResponse)` with a stream that applies read timeout per
    ///   options.
    /// - `Err(HttpError)` before the stream starts (same cases as
    ///   [`HttpClient::execute`] for the initial response).
    pub async fn execute_stream(&self, request: HttpRequest) -> HttpResult<HttpStreamResponse> {
        if !self.should_retry_request(&request) {
            return self.execute_stream_once(request).await;
        }
        self.execute_stream_with_retry(request).await
    }

    /// Performs one non-retrying streaming execution: same setup as
    /// [`HttpClient::execute_once`], but on success wraps the body in a stream
    /// with per-chunk read timeouts instead of buffering the full body.
    ///
    /// # Parameters
    /// - `request`: Built request to send (same fields as for
    ///   [`HttpClient::execute_stream`]).
    ///
    /// # Returns
    /// [`HttpStreamResponse`] or [`HttpError`].
    async fn execute_stream_once(&self, request: HttpRequest) -> HttpResult<HttpStreamResponse> {
        let url = self.resolve_url(&request)?;
        let method = request.method.clone();
        let headers = self.build_headers(&request)?;

        let body_for_log = match &request.body {
            HttpRequestBody::Bytes(bytes) | HttpRequestBody::Json(bytes) => Some(bytes.clone()),
            HttpRequestBody::Text(text) => Some(Bytes::from(text.clone())),
            HttpRequestBody::Empty => None,
        };

        let logger = HttpLogger::new(&self.options.logging, &self.options.sensitive_headers);
        logger.log_request(&method, &url, &headers, body_for_log.as_ref());

        let mut builder = self.client.request(method.clone(), url.clone());
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
            let message = format!(
                "HTTP streaming request failed with status {} for {} {}",
                response.status(),
                method,
                url
            );
            let error = response.error_for_status_ref().expect_err(
                "non-success HTTP status must produce reqwest status error via error_for_status_ref",
            );
            let mut mapped = map_reqwest_error(
                error,
                HttpErrorKind::Status,
                Some(method.clone()),
                Some(url.clone()),
            )
            .with_status(response.status());
            mapped.message = message;
            return Err(mapped);
        }

        let status = response.status();
        let response_url = response.url().clone();
        let response_headers = response.headers().clone();

        logger.log_stream_response_headers(status, &response_url, &response_headers);

        let read_timeout = self.options.timeouts.read_timeout;
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

    /// Returns whether the client should run the retry policy for this request.
    ///
    /// Retries are enabled when `max_attempts` is greater than one and the
    /// request method is allowed by [`HttpClientOptions`] retry settings.
    ///
    /// # Parameters
    /// - `request`: Request whose HTTP method is checked against the configured
    ///   retry policy.
    fn should_retry_request(&self, request: &HttpRequest) -> bool {
        self.options.retry.max_attempts > 1 && self.options.retry.allows_method(&request.method)
    }

    /// Builds a [`RetryExecutor`] from client retry options and classifies
    /// [`HttpError`] values using [`RetryHint`].
    ///
    /// # Returns
    /// Configured executor or [`HttpError`] if retry options or executor
    /// configuration is invalid.
    fn build_retry_executor(&self) -> HttpResult<RetryExecutor<HttpError>> {
        let options = RetryOptions::new(
            self.options.retry.max_attempts,
            self.options.retry.max_duration,
            self.options.retry.delay_strategy.clone(),
            Jitter::factor(self.options.retry.jitter_factor),
        )
        .map_err(|error| HttpError::other(format!("Invalid HTTP retry options: {error}")))?;

        RetryExecutor::<HttpError>::builder()
            .options(options)
            .classify_error(|error: &HttpError, _| {
                if matches!(error.retry_hint(), RetryHint::Retryable) {
                    RetryDecision::Retry
                } else {
                    RetryDecision::Abort
                }
            })
            .build()
            .map_err(|error| HttpError::other(format!("Invalid HTTP retry executor: {error}")))
    }

    /// Runs [`HttpClient::execute_once`] under the configured retry policy.
    ///
    /// # Parameters
    /// - `request`: Built request passed to each [`HttpClient::execute_once`]
    ///   attempt.
    ///
    /// # Returns
    /// Same as a successful single attempt, or a mapped [`HttpError`] when
    /// retries abort or limits are exceeded.
    async fn execute_with_retry(&self, request: HttpRequest) -> HttpResult<HttpResponse> {
        let policy = self.build_retry_executor()?;
        let client = self.clone();
        let result = policy
            .run_async(move || {
                let client = client.clone();
                let request = request.clone();
                async move { client.execute_once(request).await }
            })
            .await;
        map_retry_result(result)
    }

    /// Runs [`HttpClient::execute_stream_once`] under the configured retry
    /// policy.
    ///
    /// # Parameters
    /// - `request`: Built request passed to each
    ///   [`HttpClient::execute_stream_once`] attempt.
    ///
    /// # Returns
    /// Same as a successful single streaming attempt, or a mapped [`HttpError`]
    /// when retries abort or limits are exceeded.
    async fn execute_stream_with_retry(
        &self,
        request: HttpRequest,
    ) -> HttpResult<HttpStreamResponse> {
        let policy = self.build_retry_executor()?;
        let client = self.clone();
        let result = policy
            .run_async(move || {
                let client = client.clone();
                let request = request.clone();
                async move { client.execute_stream_once(request).await }
            })
            .await;
        map_retry_result(result)
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

        let base = self.options.base_url.as_ref().ok_or_else(|| {
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

    /// Merges default headers, injector output, and per-request headers (later
    /// wins on duplicates).
    ///
    /// # Parameters
    /// - `request`: Request supplying extra headers.
    ///
    /// # Returns
    /// Final [`HeaderMap`] or error if an injector fails.
    fn build_headers(&self, request: &HttpRequest) -> HttpResult<HeaderMap> {
        let mut headers = self.options.default_headers.clone();

        for injector in &self.injectors {
            injector.apply(&mut headers)?;
        }

        headers.extend(request.headers.clone());
        Ok(headers)
    }

    /// Sends the built request with a write-phase timeout (time to finish
    /// sending the request).
    ///
    /// # Parameters
    /// - `builder`: Reqwest request builder (method, URL, headers, body already
    ///   set).
    /// - `method`: Method for error context.
    /// - `url`: URL for error context.
    ///
    /// # Returns
    /// Raw [`reqwest::Response`] or [`HttpError`] (transport, write timeout,
    /// etc.).
    async fn send_with_write_timeout(
        &self,
        builder: reqwest::RequestBuilder,
        method: http::Method,
        url: Url,
    ) -> HttpResult<Response> {
        let timeout = self.options.timeouts.write_timeout;
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
        let timeout = self.options.timeouts.read_timeout;
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

/// Converts a [`RetryResult`] from the HTTP retry executor into [`HttpResult`].
///
/// Successful attempts pass through. Retry exhaustion and deadline failures are
/// turned into [`HttpError`] values with additional context on the message when
/// applicable.
///
/// # Parameters
/// - `result`: Outcome of the retry executor after one or more async attempts.
///
/// # Returns
/// The successful value, or an [`HttpError`] describing abort, exhaustion, or
/// deadline overrun.
fn map_retry_result<T>(result: RetryResult<T, HttpError>) -> HttpResult<T> {
    match result {
        Ok(value) => Ok(value),
        Err(RetryError::Aborted { failure, .. }) => map_retry_failure(failure),
        Err(RetryError::AttemptsExceeded {
            attempts,
            max_attempts,
            last_failure,
            ..
        }) => {
            let mut error = map_retry_failure_to_error(last_failure);
            error.message = format!(
                "{} (retry attempts exhausted: {attempts}/{max_attempts})",
                error.message
            );
            Err(error)
        }
        Err(RetryError::MaxElapsedExceeded {
            elapsed,
            max_elapsed,
            last_failure: Some(last_failure),
            ..
        }) => {
            let mut error = map_retry_failure_to_error(last_failure);
            error.message = format!(
                "{} (retry max duration exceeded: {elapsed:?}/{max_elapsed:?})",
                error.message
            );
            Err(error)
        }
        Err(RetryError::MaxElapsedExceeded {
            elapsed,
            max_elapsed,
            last_failure: None,
            ..
        }) => Err(HttpError::other(format!(
            "HTTP retry max duration exceeded before a retryable error was captured: {elapsed:?}/{max_elapsed:?}"
        ))),
    }
}

/// Maps a single retry [`AttemptFailure`] into [`HttpResult`].
///
/// # Parameters
/// - `failure`: Single attempt outcome from the retry layer.
///
/// # Returns
/// Always `Err`: either the wrapped [`HttpError`] or a synthesized timeout
/// message.
fn map_retry_failure<T>(failure: AttemptFailure<HttpError>) -> HttpResult<T> {
    Err(map_retry_failure_to_error(failure))
}

/// Converts a retry-layer attempt failure into [`HttpError`].
///
/// # Parameters
/// - `failure`: Attempt failure from the retry executor.
///
/// # Returns
/// Mapped [`HttpError`] with timeout context when applicable.
fn map_retry_failure_to_error(failure: AttemptFailure<HttpError>) -> HttpError {
    match failure {
        AttemptFailure::Error(error) => error,
        AttemptFailure::AttemptTimeout { elapsed, timeout } => HttpError::other(format!(
            "HTTP retry attempt timeout after {elapsed:?} (timeout: {timeout:?})"
        )),
    }
}

/// Maps a [`reqwest::Error`] into [`HttpError`] with best-effort
/// [`HttpErrorKind`] and optional context.
///
/// # Parameters
/// - `error`: Underlying reqwest error.
/// - `default_kind`: Kind used when reqwest does not classify the error more
///   specifically.
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
    } else if error.is_decode() {
        HttpErrorKind::Decode
    } else if error.is_status() {
        HttpErrorKind::Status
    } else if error.is_request() && error.url().is_none() {
        HttpErrorKind::InvalidUrl
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
