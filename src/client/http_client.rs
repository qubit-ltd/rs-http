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

use super::retry_controller::RetryController;
use super::sse_reconnect::SseReconnectRunner;
use crate::{
    response::HttpResponseOptions,
    sse::{SseEventStream, SseReconnectOptions},
    AsyncHttpHeaderInjector, HttpClientOptions, HttpHeaderInjector, HttpLogger, HttpRequest,
    HttpRequestBuilder, HttpRequestInterceptor, HttpRequestInterceptors, HttpResponse,
    HttpResponseInterceptor, HttpResponseInterceptors, HttpResponseMeta, HttpResult,
    HttpRetryOptions,
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
    pub(super) injectors: Vec<HttpHeaderInjector>,
    /// Async header injectors applied after sync injectors and before request-level headers.
    pub(super) async_injectors: Vec<AsyncHttpHeaderInjector>,
    /// Request interceptors applied before request send for each attempt.
    request_interceptors: HttpRequestInterceptors,
    /// Response interceptors applied on successful responses before return.
    response_interceptors: HttpResponseInterceptors,
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
            request_interceptors: HttpRequestInterceptors::new(),
            response_interceptors: HttpResponseInterceptors::new(),
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

    /// Appends a [`HttpHeaderInjector`] so its mutation function runs on every
    /// request.
    ///
    /// # Parameters
    /// - `injector`: Injector to append (order is preserved).
    ///
    /// # Returns
    /// Nothing.
    pub fn add_header_injector(&mut self, injector: HttpHeaderInjector) {
        self.injectors.push(injector);
    }

    /// Appends an async header injector whose mutation runs after sync injectors.
    ///
    /// # Parameters
    /// - `injector`: Async injector to append (order is preserved).
    ///
    /// # Returns
    /// Nothing.
    pub fn add_async_header_injector(&mut self, injector: AsyncHttpHeaderInjector) {
        self.async_injectors.push(injector);
    }

    /// Appends a request interceptor applied before each request attempt.
    ///
    /// # Parameters
    /// - `interceptor`: Request interceptor to append (order is preserved).
    pub fn add_request_interceptor(&mut self, interceptor: HttpRequestInterceptor) {
        self.request_interceptors.push(interceptor);
    }

    /// Appends a response interceptor applied on successful responses.
    ///
    /// # Parameters
    /// - `interceptor`: Response interceptor to append (order is preserved).
    pub fn add_response_interceptor(&mut self, interceptor: HttpResponseInterceptor) {
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
    pub fn clear_header_injectors(&mut self) {
        self.injectors.clear();
    }

    /// Removes all registered async header injectors.
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

    pub(crate) fn injectors_snapshot(&self) -> Vec<HttpHeaderInjector> {
        self.injectors.clone()
    }

    pub(crate) fn async_injectors_snapshot(&self) -> Vec<AsyncHttpHeaderInjector> {
        self.async_injectors.clone()
    }

    /// Sends the request and returns a unified [`HttpResponse`].
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
        let retry_options = self.options.retry.resolve(&request);
        if retry_options.should_retry(&request) {
            self.execute_with_retry(request, retry_options).await
        } else {
            self.execute_once(request).await
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
    /// request, send with write timeout, reject non-success status, return a
    /// lazily readable response.
    ///
    /// # Parameters
    /// - `request`: Built request to send (same fields as for
    ///   [`HttpClient::execute`]).
    ///
    /// # Returns
    /// [`HttpResponse`] or [`crate::HttpError`].
    pub(super) async fn execute_once(&self, request: HttpRequest) -> HttpResult<HttpResponse> {
        let mut request = request;
        self.request_interceptors.apply(&mut request)?;
        let response = self
            .prepare_and_send_once(request, "Request cancelled before sending")
            .await?;
        let mut response = response
            .into_success_or_status_error("HTTP request failed")
            .await?;
        self.response_interceptors.apply(&mut response.meta)?;
        let logger = HttpLogger::new(&self.options);
        logger.log_response(&mut response).await?;
        Ok(response)
    }

    /// Resolves URL/headers, logs request, sends one attempt, and returns a
    /// unified response.
    async fn prepare_and_send_once(
        &self,
        request: HttpRequest,
        cancellation_message: &str,
    ) -> HttpResult<HttpResponse> {
        let mut request = request;
        if let Some(error) = request.cancelled_error_if_needed(cancellation_message) {
            return Err(error);
        }
        let logger = HttpLogger::new(&self.options);
        let backend_response = request.send_impl(&self.backend, &logger).await?;
        let meta = HttpResponseMeta::new(
            backend_response.status(),
            backend_response.headers().clone(),
            backend_response.url().clone(),
            request.method().clone(),
        );
        let response_options = HttpResponseOptions::new(
            self.options.error_response_preview_limit,
            self.options.sse_json_mode,
            self.options.sse_max_line_bytes,
            self.options.sse_max_frame_bytes,
        );
        Ok(HttpResponse::from_backend(
            meta,
            backend_response,
            request.read_timeout(),
            request.cancellation_token().cloned(),
            request.resolved_url()?,
            response_options,
        ))
    }

    /// Runs [`HttpClient::execute_once`] under the configured retry policy.
    ///
    /// # Parameters
    /// - `request`: Built request passed to each [`HttpClient::execute_once`]
    ///   attempt.
    /// - `retry_options`: Effective retry options for this request.
    ///
    /// # Returns
    /// Same as a successful single attempt, or a mapped [`HttpError`] when
    /// retries abort or limits are exceeded.
    async fn execute_with_retry(
        &self,
        request: HttpRequest,
        retry_options: HttpRetryOptions,
    ) -> HttpResult<HttpResponse> {
        let honor_retry_after = request.retry_override().should_honor_retry_after();
        let retry_controller = RetryController::new(&retry_options, honor_retry_after)?;
        retry_controller.run_response(self, request).await
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
