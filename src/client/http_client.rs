/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! HTTP client: builds requests, applies defaults and interceptors, executes
//! them with optional retry, and exposes SSE helpers with reconnect.
//!
//! Single-shot execution is [`HttpClient::execute`] / [`HttpClient::execute_once`];
//! retry policy comes from [`crate::HttpClientOptions::retry`] unless overridden
//! per request.
//!
//! # Author
//!
//! Haixing Hu

use std::time::Duration;

use qubit_concurrent::Lock;
use qubit_retry::{RetryAttemptFailure, RetryError, RetryResult};

use crate::sse::SseReconnectRunner;
use crate::{
    response::HttpResponseOptions,
    sse::{SseEventStream, SseReconnectOptions},
    AsyncHttpHeaderInjector, HttpClientOptions, HttpError, HttpHeaderInjector, HttpLogger,
    HttpRequest, HttpRequestBuilder, HttpRequestInterceptor, HttpRequestInterceptors, HttpResponse,
    HttpResponseInterceptor, HttpResponseInterceptors, HttpResponseMeta, HttpResult,
    HttpRetryOptions, PendingHttpRetryAfterDelay,
};

/// High-level HTTP client: default headers, injectors, interceptors, logging,
/// timeouts, and optional per-request retry.
///
/// [`Clone`] is shallow and cheap enough for typical use (including passing into
/// retry closures); cloning does not duplicate the underlying connection pool
/// beyond what [`reqwest::Client`] already shares.
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

    /// Returns a reference to the client-wide options (timeouts, proxy, logging,
    /// default headers, retry defaults, etc.).
    ///
    /// # Returns
    /// Immutable borrow of [`HttpClientOptions`]. Never `None`; always the
    /// options installed on this client.
    pub fn options(&self) -> &HttpClientOptions {
        &self.options
    }

    /// Appends a [`HttpHeaderInjector`] so its mutation function runs on every
    /// request. Mutates `self` in place.
    ///
    /// # Parameters
    /// - `injector`: Injector to append (order is preserved).
    pub fn add_header_injector(&mut self, injector: HttpHeaderInjector) {
        self.injectors.push(injector);
    }

    /// Appends an async header injector whose mutation runs after sync injectors.
    /// Mutates `self` in place.
    ///
    /// # Parameters
    /// - `injector`: Async injector to append (order is preserved).
    pub fn add_async_header_injector(&mut self, injector: AsyncHttpHeaderInjector) {
        self.async_injectors.push(injector);
    }

    /// Appends a request interceptor run before each send attempt (including
    /// each retry attempt). Mutates `self` in place.
    ///
    /// # Parameters
    /// - `interceptor`: Request interceptor to append (order is preserved).
    pub fn add_request_interceptor(&mut self, interceptor: HttpRequestInterceptor) {
        self.request_interceptors.push(interceptor);
    }

    /// Appends a response interceptor run only after a successful HTTP status
    /// (see [`HttpClient::execute_once`]) and before response body logging.
    /// Mutates `self` in place.
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

    /// Clears all synchronous header injectors. Mutates `self` in place.
    pub fn clear_header_injectors(&mut self) {
        self.injectors.clear();
    }

    /// Clears all async header injectors. Mutates `self` in place.
    pub fn clear_async_header_injectors(&mut self) {
        self.async_injectors.clear();
    }

    /// Clears all request interceptors. Mutates `self` in place.
    pub fn clear_request_interceptors(&mut self) {
        self.request_interceptors.clear();
    }

    /// Clears all response interceptors. Mutates `self` in place.
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
    /// A new [`HttpRequestBuilder`] borrowing this client for defaults; it is
    /// not sent until built and passed to [`HttpClient::execute`] (or related
    /// APIs).
    pub fn request(&self, method: http::Method, path: &str) -> HttpRequestBuilder {
        HttpRequestBuilder::new(method, path, self)
    }

    /// Returns a clone of the client-level default header map.
    ///
    /// Used when constructing a built [`HttpRequest`] so the snapshot reflects
    /// headers at build time.
    ///
    /// # Returns
    /// Owned [`http::HeaderMap`] copy of [`HttpClientOptions`] default headers.
    pub(crate) fn headers_snapshot(&self) -> http::HeaderMap {
        self.options.default_headers.clone()
    }

    /// Returns a clone of the registered synchronous header injectors list.
    ///
    /// # Returns
    /// New [`Vec`] with the same injectors and order as on this client.
    pub(crate) fn injectors_snapshot(&self) -> Vec<HttpHeaderInjector> {
        self.injectors.clone()
    }

    /// Returns a clone of the registered async header injectors list.
    ///
    /// # Returns
    /// New [`Vec`] with the same injectors and order as on this client.
    pub(crate) fn async_injectors_snapshot(&self) -> Vec<AsyncHttpHeaderInjector> {
        self.async_injectors.clone()
    }

    /// Sends the request and returns a unified [`HttpResponse`].
    ///
    /// Chooses retry vs single attempt from resolved [`HttpRetryOptions`] for
    /// this request. Performs network I/O and may await [`HttpClient::execute_once`]
    /// multiple times with backoff between attempts when retry is enabled.
    ///
    /// # Parameters
    /// - `request`: Built request (URL resolved against `base_url` if path is
    ///   not absolute).
    ///
    /// # Returns
    /// - `Ok(HttpResponse)` when the HTTP status is success
    ///   ([`http::StatusCode::is_success`]).
    /// - `Err(HttpError)` when any attempt fails for URL/header validation,
    ///   cancellation, interceptor failure, transport/timeout, non-success
    ///   status, or when the retry executor aborts or exceeds limits (see
    ///   [`HttpClient::execute_with_retry`] and [`HttpClient::execute_once`]).
    pub async fn execute(&self, request: HttpRequest) -> HttpResult<HttpResponse> {
        let retry_options = self.options.retry.resolve(&request);
        if retry_options.should_retry(&request) {
            self.execute_with_retry(request, retry_options).await
        } else {
            self.execute_once(request).await
        }
    }

    /// Performs one non-retrying execution: request interceptors, resolve URL,
    /// merge headers, log the request, send with configured timeouts, map
    /// non-success status to an error, then response interceptors and response
    /// logging. The returned body is read lazily according to [`HttpResponse`].
    ///
    /// # Parameters
    /// - `request`: Built request to send (same fields as for
    ///   [`HttpClient::execute`]).
    ///
    /// # Returns
    /// - `Ok(HttpResponse)` on success status and after interceptors/logging
    ///   steps succeed.
    /// - `Err(HttpError)` from request/response interceptors, cancellation,
    ///   send/transport errors, status mapping, URL resolution for the response
    ///   wrapper, or response logging failures.
    ///
    /// # Side effects
    /// Network I/O, optional logging, and user-provided interceptor callbacks.
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

    /// Single low-level send: cancellation check, request logging, one backend
    /// round-trip, then wraps the backend response as [`HttpResponse`].
    ///
    /// Does not run response interceptors or success-status enforcement; those
    /// happen in [`HttpClient::execute_once`] after this returns.
    ///
    /// # Parameters
    /// - `request`: Request to send (may be mutated for logging/send path).
    /// - `cancellation_message`: Message embedded if the request is already
    ///   cancelled when this runs.
    ///
    /// # Returns
    /// - `Ok(HttpResponse)` with lazy body and metadata.
    /// - `Err(HttpError)` if cancelled before send, send fails, or
    ///   [`HttpRequest::resolved_url`] fails when building the wrapper.
    ///
    /// # Side effects
    /// Async network I/O and request logging via [`HttpLogger`].
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
            self.options.sse_done_marker_policy.clone(),
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

    /// Runs [`HttpClient::execute_once`] under the given retry policy.
    ///
    /// Between attempts may await an initial delay taken from
    /// [`PendingHttpRetryAfterDelay`] (e.g. `Retry-After`) via
    /// [`tokio::time::sleep`]. Each attempt clones the client and request.
    ///
    /// # Parameters
    /// - `request`: Built request passed to each [`HttpClient::execute_once`]
    ///   attempt (cloned per retry closure).
    /// - `options`: Effective retry options for this request (from resolution
    ///   in [`HttpClient::execute`]).
    ///
    /// # Returns
    /// - `Ok(HttpResponse)` when an attempt completes with success status.
    /// - `Err(HttpError)` from [`HttpRetryOptions::build_executor`], from any
    ///   [`HttpClient::execute_once`] failure that exhausts policy, or from
    ///   mapped [`RetryError`] (aborted, attempts exceeded, max elapsed exceeded);
    ///   see [`HttpClient::map_retry_result`].
    ///
    /// # Side effects
    /// Multiple async HTTP attempts, optional sleeps, and retry callback scheduling.
    async fn execute_with_retry(
        &self,
        request: HttpRequest,
        options: HttpRetryOptions,
    ) -> HttpResult<HttpResponse> {
        let honor_retry_after = request.retry_override().should_honor_retry_after();
        let (retry_executor, pending_after_delay) = options.build_executor(honor_retry_after)?;
        let client = self.clone();
        let result = retry_executor
            .run_async(move || {
                let client = client.clone();
                let request = request.clone();
                let pending_after_delay = pending_after_delay.clone();
                async move {
                    if let Some(delay) = pending_after_delay
                        .as_ref()
                        .and_then(Self::take_pending_after_delay)
                    {
                        tokio::time::sleep(delay).await;
                    }
                    client.execute_once(request).await
                }
            })
            .await;
        Self::map_retry_result(result)
    }

    /// Atomically takes a pending post-429 (or similar) delay, if any was set.
    ///
    /// Uses a short critical section on the shared mutex so at most one
    /// consumer removes the stored [`Duration`].
    ///
    /// # Parameters
    /// - `pending`: Shared cell optionally holding a delay to apply before the
    ///   next attempt.
    ///
    /// # Returns
    /// - `Some(duration)` if a delay was present and was taken.
    /// - `None` if no delay was set or it was already consumed.
    fn take_pending_after_delay(pending: &PendingHttpRetryAfterDelay) -> Option<Duration> {
        pending.write(Option::take)
    }

    /// Maps a finished retry run into a plain HTTP result.
    ///
    /// # Parameters
    /// - `result`: Outcome from the generic retry executor (`T` is normally
    ///   [`HttpResponse`] here).
    ///
    /// # Returns
    /// - `Ok(value)` on successful completion of the retried operation.
    /// - `Err(HttpError)` with messages/source wired from [`RetryError`] variants.
    fn map_retry_result<T>(result: RetryResult<T, HttpError>) -> HttpResult<T> {
        match result {
            Ok(value) => Ok(value),
            Err(RetryError::Aborted {
                attempts,
                elapsed,
                failure,
            }) => Err(Self::map_retry_aborted(attempts, elapsed, failure)),
            Err(RetryError::AttemptsExceeded {
                attempts,
                max_attempts,
                last_failure,
                ..
            }) => {
                let mut error = Self::map_retry_attempt_failure(last_failure);
                error.message = format!(
                    "{} (retry attempts exhausted: {attempts}/{max_attempts})",
                    error.message
                );
                Err(error)
            }
            Err(RetryError::MaxElapsedExceeded {
                elapsed,
                max_elapsed,
                last_failure,
                ..
            }) => Err(Self::map_retry_max_elapsed_exceeded(
                elapsed,
                max_elapsed,
                last_failure,
            )),
        }
    }

    /// Converts a single-attempt retry failure into [`HttpError`].
    ///
    /// # Parameters
    /// - `failure`: Either a wrapped [`HttpError`] or an attempt-timeout marker.
    ///
    /// # Returns
    /// The same error for `Error`, or a retry-attempt-timeout error for
    /// [`RetryAttemptFailure::AttemptTimeout`].
    fn map_retry_attempt_failure(failure: RetryAttemptFailure<HttpError>) -> HttpError {
        match failure {
            RetryAttemptFailure::Error(error) => error,
            RetryAttemptFailure::AttemptTimeout { elapsed, timeout } => {
                HttpError::retry_attempt_timeout(format!(
                    "HTTP retry attempt timeout after {elapsed:?} (timeout: {timeout:?})"
                ))
            }
        }
    }

    /// Builds the error returned when retry stops early (aborted).
    ///
    /// # Parameters
    /// - `attempts`: Number of attempts before abort.
    /// - `elapsed`: Total elapsed retry time.
    /// - `failure`: Last attempt failure or timeout.
    ///
    /// # Returns
    /// [`HttpError::retry_aborted`] with optional chained source for HTTP
    /// failures, or attempt-timeout variant when the failure was a timeout.
    fn map_retry_aborted(
        attempts: u32,
        elapsed: Duration,
        failure: RetryAttemptFailure<HttpError>,
    ) -> HttpError {
        match failure {
            RetryAttemptFailure::Error(error) => {
                let summary = error.message.clone();
                HttpError::retry_aborted(format!(
                    "HTTP retry aborted after {attempts} attempt(s) in {elapsed:?}: {summary}"
                ))
                .with_source(error)
            }
            RetryAttemptFailure::AttemptTimeout { elapsed, timeout } => {
                HttpError::retry_attempt_timeout(format!(
                    "HTTP retry attempt timeout after {elapsed:?} (timeout: {timeout:?})"
                ))
            }
        }
    }

    /// Builds the error when total retry duration exceeds the configured cap.
    ///
    /// # Parameters
    /// - `elapsed`: Time spent in the retry loop.
    /// - `max_elapsed`: Configured maximum wall time for retries.
    /// - `last_failure`: Last captured attempt failure, if any.
    ///
    /// # Returns
    /// Augments the last failure message when present, otherwise a dedicated
    /// max-elapsed error with no underlying attempt error.
    fn map_retry_max_elapsed_exceeded(
        elapsed: Duration,
        max_elapsed: Duration,
        last_failure: Option<RetryAttemptFailure<HttpError>>,
    ) -> HttpError {
        match last_failure {
            Some(last_failure) => {
                let mut error = Self::map_retry_attempt_failure(last_failure);
                error.message = format!(
                    "{} (retry max duration exceeded: {elapsed:?}/{max_elapsed:?})",
                    error.message
                );
                error
            }
            None => HttpError::retry_max_elapsed_exceeded(format!(
                "HTTP retry max duration exceeded before a retryable error was captured: {elapsed:?}/{max_elapsed:?}"
            )),
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
    /// - `options`: Reconnect limits and delay policy.
    ///
    /// # Returns
    /// SSE event stream yielding events from one or more reconnect sessions.
    ///
    /// # Errors
    /// Stream items are `Result`; `Err` covers per-item failures such as:
    /// - initial stream-open failures when not reconnectable or retries exhausted;
    /// - SSE protocol errors (non-reconnectable by default);
    /// - transport/read errors after reconnect budget is exhausted.
    ///
    /// # Side effects
    /// Performs repeated HTTP requests and reads on reconnect; may sleep between
    /// attempts according to reconnect options.
    pub fn execute_sse_with_reconnect(
        &self,
        request: HttpRequest,
        options: SseReconnectOptions,
    ) -> SseEventStream {
        SseReconnectRunner::new(self.clone(), request, options).run()
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
