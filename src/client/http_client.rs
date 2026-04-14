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
use futures_util::StreamExt;
use http::{HeaderMap, StatusCode};
use qubit_function::MutatingFunction;
use url::Url;

use super::error_mapper::{map_reqwest_error, ReqwestErrorPhase};
use super::request_pipeline::{PreparedRequestSend, RequestPipeline};
use super::retry_controller::RetryController;
use super::sse_reconnect::SseReconnectRunner;
use crate::{
    sse::{SseEventStream, SseReconnectOptions},
    AsyncHeaderInjector, HeaderInjector, HttpClientOptions, HttpError, HttpErrorKind, HttpLogger,
    HttpRequest, HttpRequestBuilder, HttpResponse, HttpResult, HttpRetryOptions,
    HttpStreamResponse, RequestInterceptor, ResponseInterceptor,
};

/// High-level HTTP client that applies options, header injection, logging, and timeouts.
#[derive(Clone)]
pub struct HttpClient {
    /// Pluggable low-level HTTP stack used to send requests (currently reqwest).
    pub(super) backend: reqwest::Client,
    /// Timeouts, proxy, logging, default headers, and related settings.
    pub(super) options: HttpClientOptions,
    /// Header injectors applied to every outgoing request after default
    /// headers.
    pub(super) injectors: Vec<HeaderInjector>,
    /// Async header injectors applied after sync injectors and before request-level headers.
    pub(super) async_injectors: Vec<AsyncHeaderInjector>,
    /// Request interceptors applied before request send for each attempt.
    request_interceptors: Vec<RequestInterceptor>,
    /// Response interceptors applied on successful responses before return.
    response_interceptors: Vec<ResponseInterceptor>,
}

impl HttpClient {
    /// Wraps a built [`reqwest::Client`] with the given options and an empty
    /// injector list.
    ///
    /// # Parameters
    /// - `backend`: Configured low-level HTTP client used for I/O.
    /// - `options`: Client-wide timeouts, headers, proxy, logging, etc.
    ///
    /// # Returns
    /// A new [`HttpClient`] with no injectors until
    /// [`HttpClient::add_header_injector`] is called.
    pub(crate) fn new(backend: reqwest::Client, options: HttpClientOptions) -> Self {
        Self {
            backend,
            options,
            injectors: Vec::new(),
            async_injectors: Vec::new(),
            request_interceptors: Vec::new(),
            response_interceptors: Vec::new(),
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

    /// Appends an async header injector whose mutation runs after sync injectors.
    ///
    /// # Parameters
    /// - `injector`: Async injector to append (order is preserved).
    ///
    /// # Returns
    /// Nothing.
    pub fn add_async_header_injector(&mut self, injector: AsyncHeaderInjector) {
        self.async_injectors.push(injector);
    }

    /// Appends a request interceptor applied before each request attempt.
    ///
    /// # Parameters
    /// - `interceptor`: Request interceptor to append (order is preserved).
    pub fn add_request_interceptor(&mut self, interceptor: RequestInterceptor) {
        self.request_interceptors.push(interceptor);
    }

    /// Appends a response interceptor applied on successful responses.
    ///
    /// # Parameters
    /// - `interceptor`: Response interceptor to append (order is preserved).
    pub fn add_response_interceptor(&mut self, interceptor: ResponseInterceptor) {
        self.response_interceptors.push(interceptor);
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

    /// Removes all registered async header injectors.
    ///
    /// # Returns
    /// Nothing.
    pub fn clear_async_header_injectors(&mut self) {
        self.async_injectors.clear();
    }

    /// Removes all registered request interceptors.
    pub fn clear_request_interceptors(&mut self) {
        self.request_interceptors.clear();
    }

    /// Removes all registered response interceptors.
    pub fn clear_response_interceptors(&mut self) {
        self.response_interceptors.clear();
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
        HttpRequestBuilder::new(method, path, self)
    }

    pub(crate) fn headers_snapshot(&self) -> http::HeaderMap {
        self.options.default_headers.clone()
    }

    pub(crate) fn injectors_snapshot(&self) -> Vec<HeaderInjector> {
        self.injectors.clone()
    }

    pub(crate) fn async_injectors_snapshot(&self) -> Vec<AsyncHeaderInjector> {
        self.async_injectors.clone()
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
        let retry_options = self.resolve_retry_options(&request);
        let honor_retry_after = request.retry_override().should_honor_retry_after();
        if self.should_retry_request(&request, &retry_options) {
            self.execute_with_retry(request, retry_options, honor_retry_after)
                .await
        } else {
            self.execute_once(request).await
        }
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
        let retry_options = self.resolve_retry_options(&request);
        let honor_retry_after = request.retry_override().should_honor_retry_after();
        if self.should_retry_request(&request, &retry_options) {
            self.execute_stream_with_retry(request, retry_options, honor_retry_after)
                .await
        } else {
            self.execute_stream_once(request).await
        }
    }

    /// Opens an SSE stream and reconnects automatically on retryable stream
    /// failures.
    ///
    /// Reconnect behavior:
    /// - retryable transport/read failures trigger reconnects;
    /// - optional reconnect on clean EOF (`reconnect_on_eof`);
    /// - `Last-Event-ID` is set from the latest parsed SSE `id:` field;
    /// - optional use of SSE `retry:` as next reconnect delay.
    ///
    /// # Parameters
    /// - `request`: SSE request template reused on reconnect.
    /// - `reconnect_options`: Reconnect limits and delay policy.
    ///
    /// # Returns
    /// SSE event stream yielding events from one or more reconnect sessions.
    ///
    /// # Errors
    /// Per-item stream errors include:
    /// - initial stream-open failures (when not reconnectable or retries exhausted);
    /// - SSE protocol errors (non-reconnectable by default);
    /// - transport/read errors after reconnect budget is exhausted.
    pub fn execute_sse_with_reconnect(
        &self,
        request: HttpRequest,
        reconnect_options: SseReconnectOptions,
    ) -> SseEventStream {
        SseReconnectRunner::new(self.clone(), request, reconnect_options).run()
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
    pub(super) async fn execute_once(&self, request: HttpRequest) -> HttpResult<HttpResponse> {
        let mut request = request;
        self.apply_request_interceptors(&mut request)?;
        let pipeline = RequestPipeline::new(self);
        let PreparedRequestSend {
            method,
            url,
            cancellation_token,
            response,
        } = pipeline
            .prepare_and_send_once(request, "Request cancelled before sending")
            .await?;

        let response = pipeline
            .ensure_success_response(response, &method, &url, "HTTP request failed")
            .await?;

        let status = response.status();
        let response_url = response.url().clone();
        let response_headers = response.headers().clone();
        self.apply_response_interceptors(status, &response_headers, &method, &response_url)?;

        let body = pipeline
            .read_body_with_timeout(
                response,
                method.clone(),
                response_url.clone(),
                cancellation_token.as_ref(),
            )
            .await?;

        let logger = HttpLogger::new(&self.options);
        logger.log_response(status, &response_url, &response_headers, &body);

        Ok(HttpResponse::new(
            status,
            response_headers,
            body,
            response_url,
        ))
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
    pub(super) async fn execute_stream_once(
        &self,
        request: HttpRequest,
    ) -> HttpResult<HttpStreamResponse> {
        let mut request = request;
        self.apply_request_interceptors(&mut request)?;
        let pipeline = RequestPipeline::new(self);
        let PreparedRequestSend {
            method,
            url,
            cancellation_token,
            response,
        } = pipeline
            .prepare_and_send_once(request, "Streaming request cancelled before sending")
            .await?;

        let response = pipeline
            .ensure_success_response(response, &method, &url, "HTTP streaming request failed")
            .await?;

        let status = response.status();
        let response_url = response.url().clone();
        let response_headers = response.headers().clone();
        self.apply_response_interceptors(status, &response_headers, &method, &response_url)?;

        let logger = HttpLogger::new(&self.options);
        logger.log_stream_response_headers(status, &response_url, &response_headers);

        let read_timeout = self.options.timeouts.read_timeout;
        let method_for_err = method.clone();
        let url_for_err = response_url.clone();
        let cancellation_token_for_stream = cancellation_token.clone();

        let mut stream = response.bytes_stream();
        let wrapped = stream! {
            loop {
                let next = if let Some(token) = &cancellation_token_for_stream {
                    tokio::select! {
                        _ = token.cancelled() => {
                            yield Err(HttpError::cancelled("Streaming response cancelled while reading body")
                                .with_method(method_for_err.clone())
                                .with_url(url_for_err.clone()));
                            break;
                        }
                        item = tokio::time::timeout(read_timeout, stream.next()) => item,
                    }
                } else {
                    tokio::time::timeout(read_timeout, stream.next()).await
                };
                match next {
                    Ok(Some(Ok(bytes))) => yield Ok(bytes),
                    Ok(Some(Err(error))) => {
                        let mapped = map_reqwest_error(
                            error,
                            HttpErrorKind::Transport,
                            Some(ReqwestErrorPhase::Read),
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

        Ok(HttpStreamResponse::new_with_sse_options(
            status,
            response_headers,
            response_url,
            Box::pin(wrapped),
            self.options.sse_json_mode,
            self.options.sse_max_line_bytes,
            self.options.sse_max_frame_bytes,
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
    /// - `retry_options`: Effective retry options after applying request-level overrides.
    fn should_retry_request(
        &self,
        request: &HttpRequest,
        retry_options: &HttpRetryOptions,
    ) -> bool {
        retry_options.max_attempts > 1 && retry_options.allows_method(request.method())
    }

    /// Resolves request-level retry override against client-level retry options.
    ///
    /// # Parameters
    /// - `request`: Request whose override is applied.
    ///
    /// # Returns
    /// Effective retry options for this request.
    fn resolve_retry_options(&self, request: &HttpRequest) -> HttpRetryOptions {
        let mut options = self.options.retry.clone();
        options.enabled = request.retry_override().resolve_enabled(options.enabled);
        options.method_policy = request
            .retry_override()
            .resolve_method_policy(options.method_policy);
        options
    }

    /// Applies registered request interceptors in insertion order.
    ///
    /// # Parameters
    /// - `request`: Request snapshot to mutate before URL resolution and send.
    ///
    /// # Returns
    /// `Ok(())` when all interceptors succeed.
    ///
    /// # Errors
    /// Returns the first interceptor error and enriches it with method/URL
    /// context when missing.
    fn apply_request_interceptors(&self, request: &mut HttpRequest) -> HttpResult<()> {
        for interceptor in &self.request_interceptors {
            interceptor.apply(request).map_err(|error| {
                let mut mapped = error;
                if mapped.method.is_none() {
                    mapped = mapped.with_method(request.method().clone());
                }
                if mapped.url.is_none() {
                    if let Ok(parsed_url) = Url::parse(request.path()) {
                        mapped = mapped.with_url(parsed_url);
                    }
                }
                mapped
            })?;
        }
        Ok(())
    }

    /// Applies registered response interceptors in insertion order.
    ///
    /// # Parameters
    /// - `status`: Response status code.
    /// - `headers`: Response headers.
    /// - `method`: Request method.
    /// - `url`: Response URL.
    ///
    /// # Returns
    /// `Ok(())` when all interceptors accept the response.
    ///
    /// # Errors
    /// Returns the first interceptor error and enriches it with
    /// status/method/URL context when missing.
    fn apply_response_interceptors(
        &self,
        status: StatusCode,
        headers: &HeaderMap,
        method: &http::Method,
        url: &Url,
    ) -> HttpResult<()> {
        for interceptor in &self.response_interceptors {
            interceptor
                .apply(status, headers, method, url)
                .map_err(|error| {
                    let mut mapped = error;
                    if mapped.status.is_none() {
                        mapped = mapped.with_status(status);
                    }
                    if mapped.method.is_none() {
                        mapped = mapped.with_method(method.clone());
                    }
                    if mapped.url.is_none() {
                        mapped = mapped.with_url(url.clone());
                    }
                    mapped
                })?;
        }
        Ok(())
    }

    /// Runs [`HttpClient::execute_once`] under the configured retry policy.
    ///
    /// # Parameters
    /// - `request`: Built request passed to each [`HttpClient::execute_once`]
    ///   attempt.
    /// - `retry_options`: Effective retry options for this request.
    /// - `honor_retry_after`: Whether to honor `Retry-After` on retryable
    ///   status responses (`429` and `5xx`).
    ///
    /// # Returns
    /// Same as a successful single attempt, or a mapped [`HttpError`] when
    /// retries abort or limits are exceeded.
    async fn execute_with_retry(
        &self,
        request: HttpRequest,
        retry_options: HttpRetryOptions,
        honor_retry_after: bool,
    ) -> HttpResult<HttpResponse> {
        let retry_controller = RetryController::new(&retry_options, honor_retry_after)?;
        retry_controller.run_response(self, request).await
    }

    /// Runs [`HttpClient::execute_stream_once`] under the configured retry
    /// policy.
    ///
    /// # Parameters
    /// - `request`: Built request passed to each
    ///   [`HttpClient::execute_stream_once`] attempt.
    /// - `retry_options`: Effective retry options for this request.
    /// - `honor_retry_after`: Whether to honor `Retry-After` on retryable
    ///   status responses (`429` and `5xx`).
    ///
    /// # Returns
    /// Same as a successful single streaming attempt, or a mapped [`HttpError`]
    /// when retries abort or limits are exceeded.
    async fn execute_stream_with_retry(
        &self,
        request: HttpRequest,
        retry_options: HttpRetryOptions,
        honor_retry_after: bool,
    ) -> HttpResult<HttpStreamResponse> {
        let retry_controller = RetryController::new(&retry_options, honor_retry_after)?;
        retry_controller.run_stream(self, request).await
    }
}

impl std::fmt::Debug for HttpClient {
    /// Formats the client for debugging (exposes options and injectors; omits
    /// the backend client).
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
            .field("async_injectors", &self.async_injectors)
            .field("request_interceptors", &self.request_interceptors)
            .field("response_interceptors", &self.response_interceptors)
            .finish_non_exhaustive()
    }
}
