// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! HTTP client: builds requests, applies defaults and interceptors, executes
//! them with optional retry, and exposes SSE helpers with reconnect.
//!
//! Single-shot execution is [`HttpClient::execute`] /
//! [`HttpClient::execute_once`]; retry policy comes from
//! [`crate::HttpClientOptions::retry`] unless overridden per request.

use std::time::Duration;
use std::time::Instant;

use qubit_redact::Redactor;
use qubit_retry::AttemptFailure;
use qubit_retry::Retry;
use qubit_retry::RetryDecision;
use qubit_retry::RetryError;
use qubit_retry::RetryFailure;
use qubit_retry::RetryLimitKind;
use qubit_retry::RetryRule;
use qubit_retry::RetryTimeoutScope;

use super::internal::HttpAttemptExecutionContext;
use super::internal::HttpAttemptResponse;
use crate::AsyncHttpHeaderInjector;
use crate::HttpClientOptions;
use crate::HttpError;
use crate::HttpHeaderInjector;
use crate::HttpLogger;
use crate::HttpRequest;
use crate::HttpRequestBuilder;
use crate::HttpRequestInterceptor;
use crate::HttpRequestInterceptors;
use crate::HttpResponse;
use crate::HttpResponseInterceptor;
use crate::HttpResponseInterceptors;
use crate::HttpResponseMeta;
use crate::HttpResult;
use crate::HttpRetryOptions;
use crate::response::HttpResponseOptions;
use crate::sse::SseMessageStream;
use crate::sse::SseReconnectOptions;
use crate::sse::SseReconnectRunner;

/// High-level HTTP client: default headers, injectors, interceptors, logging,
/// timeouts, and optional per-request retry.
///
/// [`Clone`] is shallow and cheap enough for typical use (including passing
/// into retry closures); cloning does not duplicate the underlying connection
/// pool beyond what [`reqwest::Client`] already shares.
#[derive(Clone)]
pub struct HttpClient {
    /// Pluggable low-level HTTP stack used to send requests (currently
    /// reqwest).
    pub(super) backend: reqwest::Client,
    /// Timeouts, proxy, logging, default headers, and related settings.
    pub(super) options: HttpClientOptions,
    /// Shared redactor for all request/response/error snapshots created by
    /// this client.
    log_redactor: Redactor,
    /// Header injectors applied to every outgoing request after default
    /// headers.
    pub(super) injectors: Vec<HttpHeaderInjector>,
    /// Async header injectors applied after sync injectors and before
    /// request-level headers.
    pub(super) async_injectors: Vec<AsyncHttpHeaderInjector>,
    /// Request interceptors applied before request send for each attempt.
    request_interceptors: HttpRequestInterceptors,
    /// Response interceptors applied on successful responses before return.
    response_interceptors: HttpResponseInterceptors,
}

struct HttpRetryRule {
    options: HttpRetryOptions,
    honor_retry_after: bool,
}

impl RetryRule<HttpError> for HttpRetryRule {
    fn decide(
        &self,
        failure: &AttemptFailure<HttpError>,
        _context: &qubit_retry::RetryContext,
    ) -> RetryDecision {
        let AttemptFailure::Error(error) = failure else {
            return RetryDecision::UseDefault;
        };
        if !HttpClient::is_retryable_error(error, &self.options) {
            return RetryDecision::Abort;
        }
        if self.honor_retry_after {
            if let Some(delay) = error.retry_after {
                return RetryDecision::RetryWithHint(delay);
            }
        }
        RetryDecision::Retry
    }
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
    pub(crate) fn new(
        backend: reqwest::Client,
        options: HttpClientOptions,
    ) -> Self {
        let log_redactor = Redactor::new(options.log_redaction_policy.clone());
        Self {
            backend,
            options,
            log_redactor,
            injectors: Vec::new(),
            async_injectors: Vec::new(),
            request_interceptors: HttpRequestInterceptors::new(),
            response_interceptors: HttpResponseInterceptors::new(),
        }
    }

    /// Returns a reference to the client-wide options (timeouts, proxy,
    /// logging, default headers, retry defaults, etc.).
    ///
    /// # Returns
    /// Immutable borrow of [`HttpClientOptions`]. Never `None`; always the
    /// options installed on this client.
    pub fn options(&self) -> &HttpClientOptions {
        &self.options
    }

    /// Returns the shared [`Redactor`] used by this client.
    pub(crate) fn log_redactor(&self) -> &Redactor {
        &self.log_redactor
    }

    /// Appends a [`HttpHeaderInjector`] so its mutation function runs on every
    /// request. Mutates `self` in place.
    ///
    /// # Parameters
    /// - `injector`: Injector to append (order is preserved).
    pub fn add_header_injector(&mut self, injector: HttpHeaderInjector) {
        self.injectors.push(injector);
    }

    /// Appends an async header injector whose mutation runs after sync
    /// injectors. Mutates `self` in place.
    ///
    /// # Parameters
    /// - `injector`: Async injector to append (order is preserved).
    pub fn add_async_header_injector(
        &mut self,
        injector: AsyncHttpHeaderInjector,
    ) {
        self.async_injectors.push(injector);
    }

    /// Appends a request interceptor run before each send attempt (including
    /// each retry attempt). Mutates `self` in place.
    ///
    /// # Parameters
    /// - `interceptor`: Request interceptor to append (order is preserved).
    pub fn add_request_interceptor(
        &mut self,
        interceptor: HttpRequestInterceptor,
    ) {
        self.request_interceptors.push(interceptor);
    }

    /// Appends a response interceptor run only after a successful HTTP status
    /// (after the internal `execute_once` step) and before response body
    /// logging. Mutates `self` in place.
    ///
    /// # Parameters
    /// - `interceptor`: Response interceptor to append (order is preserved).
    pub fn add_response_interceptor(
        &mut self,
        interceptor: HttpResponseInterceptor,
    ) {
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
    pub fn add_header(
        &mut self,
        name: &str,
        value: &str,
    ) -> HttpResult<&mut Self> {
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
    pub fn add_headers(
        &mut self,
        headers: &[(&str, &str)],
    ) -> HttpResult<&mut Self> {
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
    pub fn request(
        &self,
        method: http::Method,
        path: &str,
    ) -> HttpRequestBuilder {
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
    pub(crate) fn async_injectors_snapshot(
        &self,
    ) -> Vec<AsyncHttpHeaderInjector> {
        self.async_injectors.clone()
    }

    /// Sends the request and returns a unified [`HttpResponse`].
    ///
    /// Chooses retry vs single attempt from resolved [`HttpRetryOptions`] for
    /// this request. Performs network I/O and may await the internal
    /// `execute_once` path
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
    ///   status, or when the retry executor aborts or exceeds limits.
    pub async fn execute(
        &self,
        request: HttpRequest,
    ) -> HttpResult<HttpResponse> {
        let retry_options = self.options.retry.resolve(&request);
        let result = if retry_options.should_retry(&request) {
            self.execute_with_retry(request, retry_options).await
        } else {
            self.execute_once(request).await
        };
        result
            .map_err(|error| error.with_log_redactor(self.log_redactor.clone()))
    }

    /// Performs one non-retrying execution: pre-send cancellation check,
    /// request interceptors, resolve URL, merge headers, log the request, send
    /// with configured timeouts, map non-success status to an error, then
    /// response interceptors and response logging. The returned body is read
    /// lazily according to [`HttpResponse`].
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
    pub(crate) async fn execute_once(
        &self,
        request: HttpRequest,
    ) -> HttpResult<HttpResponse> {
        self.execute_once_with_context(
            request,
            HttpAttemptExecutionContext::direct(),
        )
        .await
        .map(|attempt| attempt.into_parts().0)
    }

    /// Performs one attempt with private cancellation ownership routing.
    ///
    /// # Parameters
    /// - `request`: Request snapshot consumed by this attempt.
    /// - `context`: Non-escaping cancellation ownership for direct or retry
    ///   execution.
    ///
    /// # Returns
    /// A successful response plus the retry-flow token restoration decision.
    ///
    /// # Errors
    /// Returns [`HttpError`] for interceptor failures, cancellation, request
    /// preparation or transport failures, unsuccessful status mapping, and
    /// response logging failures.
    async fn execute_once_with_context(
        &self,
        request: HttpRequest,
        context: HttpAttemptExecutionContext,
    ) -> HttpResult<HttpAttemptResponse> {
        let result = async {
            let mut request = request;
            if let Some(error) = request.cancelled_error_if_needed(
                "Request cancelled before sending",
                context.io_cancellation_token(&request),
            ) {
                return Err(error);
            }
            self.request_interceptors.apply(&mut request)?;
            let restore_retry_flow_token =
                context.should_restore_retry_flow_token(&request);
            let response = self
                .prepare_and_send_once(
                    request,
                    "Request cancelled before sending",
                    context,
                )
                .await?;
            let mut response = response
                .into_success_or_status_error("HTTP request failed")
                .await?;
            self.response_interceptors.apply(&mut response.meta)?;
            let logger = HttpLogger::from_options_with_redactor(
                &self.options,
                self.log_redactor.clone(),
            );
            logger.log_response(&mut response).await?;
            Ok(HttpAttemptResponse::new(response, restore_retry_flow_token))
        }
        .await;
        result
            .map_err(|error| error.with_log_redactor(self.log_redactor.clone()))
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
    /// - `context`: Non-escaping cancellation ownership for this attempt.
    ///
    /// # Returns
    /// - `Ok(HttpResponse)` with lazy body and metadata.
    /// - `Err(HttpError)` if cancelled before send, URL resolution fails, or
    ///   send fails.
    ///
    /// # Side effects
    /// Async network I/O and request logging via [`HttpLogger`].
    async fn prepare_and_send_once(
        &self,
        request: HttpRequest,
        cancellation_message: &str,
        context: HttpAttemptExecutionContext,
    ) -> HttpResult<HttpResponse> {
        let mut request = request;
        let cancellation_token =
            context.io_cancellation_token(&request).cloned();
        if let Some(error) = request.cancelled_error_if_needed(
            cancellation_message,
            cancellation_token.as_ref(),
        ) {
            return Err(error);
        }
        let logger = HttpLogger::from_options_with_redactor(
            &self.options,
            self.log_redactor.clone(),
        );
        let request_url = request.resolved_url()?;
        let backend_response = request
            .send_impl(&self.backend, &logger, cancellation_token.clone())
            .await?;
        let log_redactor = request.log_redactor().clone();
        let meta = HttpResponseMeta::new(
            backend_response.status(),
            backend_response.headers().clone(),
            backend_response.url().clone(),
            request.method().clone(),
        )
        .with_log_redactor(log_redactor.clone());
        let response_options = HttpResponseOptions::from_client_options(
            &self.options,
            log_redactor,
        );
        Ok(HttpResponse::from_backend(
            meta,
            backend_response,
            request.read_timeout(),
            cancellation_token,
            request_url,
            response_options,
        ))
    }

    /// Runs [`HttpClient::execute_once`] under the given retry policy.
    ///
    /// Between attempts waits according to the resolved retry delay, optionally
    /// honoring `Retry-After` by extending the next sleep. Each attempt clones
    /// the request so request bodies can be rebuilt when supported.
    ///
    /// # Parameters
    /// - `request`: Built request passed to each [`HttpClient::execute_once`]
    ///   attempt (cloned per retry closure).
    /// - `options`: Effective retry options for this request (from resolution
    ///   in [`HttpClient::execute`]).
    ///
    /// # Returns
    /// - `Ok(HttpResponse)` when an attempt completes with success status.
    /// - `Err(HttpError)` from any [`HttpClient::execute_once`] failure that is
    ///   non-retryable, or from retry exhaustion/max-duration enforcement.
    ///
    /// # Side effects
    /// Multiple async HTTP attempts and optional sleeps.
    async fn execute_with_retry(
        &self,
        request: HttpRequest,
        options: HttpRetryOptions,
    ) -> HttpResult<HttpResponse> {
        let honor_retry_after =
            request.retry_override().should_honor_retry_after();
        let retry_policy = options.to_executor_policy();
        let started_at = Instant::now();

        let retry_policy = Retry::<HttpError>::builder(retry_policy)
            .rule(HttpRetryRule {
                options: options.clone(),
                honor_retry_after,
            })
            .build();

        let cancellation_token = request.cancellation_token().cloned();
        let attempt_context = HttpAttemptExecutionContext::retry(&request);
        let request_method = request.method().clone();
        let request_url = request.resolved_url().ok();
        let retry_request = request.clone();
        let mut async_retry = retry_policy.asynchronous();
        if let Some(token) = cancellation_token.as_ref() {
            async_retry = async_retry.cancellation_token(token.clone());
        }
        let retry_result = async_retry
            .run(|| {
                let attempt_request = retry_request.clone();
                let attempt_context = attempt_context.clone();
                async move {
                    self.execute_once_with_context(
                        attempt_request,
                        attempt_context,
                    )
                    .await
                }
            })
            .await;

        match retry_result {
            Ok(response) => {
                let (mut response, restore_retry_flow_token) =
                    response.into_value().into_parts();
                if restore_retry_flow_token {
                    if let Some(token) = cancellation_token {
                        response.set_cancellation_token(token);
                    }
                }
                Ok(response)
            }
            Err(error) => Err(Self::map_retry_error(
                error,
                started_at,
                options.max_duration,
                options.max_attempts,
                &request_method,
                request_url.as_ref(),
            )),
        }
    }

    /// Returns whether `error` is retryable under `options`.
    ///
    /// # Parameters
    /// - `error`: Error produced by a single HTTP attempt.
    /// - `options`: Effective retry options for the request.
    ///
    /// # Returns
    /// `true` if another attempt may be scheduled.
    fn is_retryable_error(
        error: &HttpError,
        options: &HttpRetryOptions,
    ) -> bool {
        if error.kind == crate::HttpErrorKind::Status {
            error
                .status
                .is_some_and(|status| options.is_retryable_status(status))
        } else {
            options.is_retryable_error_kind(error.kind)
        }
    }

    /// Adds retry-attempt exhaustion context to the last attempt error.
    ///
    /// # Parameters
    /// - `error`: Last retryable attempt error.
    /// - `attempts`: Number of attempts already made.
    /// - `max_attempts`: Configured maximum attempts.
    ///
    /// # Returns
    /// The same error with retry exhaustion details appended to its message.
    fn map_retry_attempts_exhausted(
        mut error: HttpError,
        attempts: u32,
        max_attempts: u32,
    ) -> HttpError {
        error.message = format!(
            "{} (retry attempts exhausted: {attempts}/{max_attempts})",
            error.message
        );
        error
    }

    /// Builds the error returned when retry policy stops early.
    ///
    /// # Parameters
    /// - `error`: Attempt error that the retry policy chose not to retry.
    /// - `attempts`: Number of attempts already made.
    /// - `started_at`: Start instant of the retry flow.
    ///
    /// # Returns
    /// [`HttpError::retry_aborted`] with the original [`HttpError`] chained as
    /// source for callers that need the underlying status or transport error.
    fn map_retry_aborted(
        error: HttpError,
        attempts: u32,
        started_at: Instant,
    ) -> HttpError {
        let elapsed = started_at.elapsed();
        let summary = error.message.clone();
        HttpError::retry_aborted(format!(
            "HTTP retry aborted after {attempts} attempt(s) in {elapsed:?}: {summary}"
        ))
        .with_source(error)
    }

    /// Builds the error when retry max-duration is exhausted.
    ///
    /// # Parameters
    /// - `started_at`: Start instant of the retry flow.
    /// - `max_duration`: Configured max-duration budget.
    /// - `last_error`: Last captured retryable attempt error, if any.
    ///
    /// # Returns
    /// Augments the last failure when present, otherwise a dedicated
    /// max-duration error with no underlying attempt error.
    fn map_retry_max_duration_exceeded(
        started_at: Instant,
        max_duration: Duration,
        last_error: Option<HttpError>,
    ) -> HttpError {
        let elapsed = started_at.elapsed();
        let max_duration_text = format!("{max_duration:?}");
        match last_error {
            Some(mut error) => {
                error.message = format!(
                    "{} (retry max duration exceeded: {elapsed:?}/{max_duration_text})",
                    error.message
                );
                error
            }
            None => HttpError::retry_max_elapsed_exceeded(format!(
                "HTTP retry max duration exceeded before a retryable error was captured: {elapsed:?}/{max_duration_text}"
            )),
        }
    }

    /// Maps a [`qubit_retry::RetryError`] into this crate's HTTP error model.
    ///
    /// # Parameters
    /// - `error`: Terminal retry error from `qubit-retry`.
    /// - `started_at`: Monotonic start instant of the HTTP retry flow.
    /// - `max_duration`: Optional HTTP total retry budget.
    /// - `max_attempts`: Configured maximum HTTP attempts.
    /// - `request_method`: Request method attached to cancellation errors.
    /// - `request_url`: Resolved URL attached to cancellation errors.
    ///
    /// # Returns
    /// A rich [`HttpError`] preserving the last attempt error when available.
    fn map_retry_error(
        error: RetryError<HttpError>,
        started_at: Instant,
        max_duration: Option<Duration>,
        max_attempts: u32,
        request_method: &http::Method,
        request_url: Option<&url::Url>,
    ) -> HttpError {
        let attempts = error.context().attempts();
        match error.failure() {
            RetryFailure::Cancelled { phase, .. } => {
                let message = format!("HTTP retry cancelled during {phase}");
                return Self::retry_cancelled_error(
                    &message,
                    request_method,
                    request_url,
                )
                .with_source(error);
            }
            RetryFailure::TimedOut { scope, .. } => {
                let message = format!(
                    "HTTP retry {scope} timed out after {attempts} attempt(s)"
                );
                return match scope {
                    RetryTimeoutScope::Attempt => {
                        HttpError::retry_attempt_timeout(message)
                            .with_source(error)
                    }
                    RetryTimeoutScope::Flow => {
                        HttpError::retry_max_elapsed_exceeded(message)
                            .with_source(error)
                    }
                };
            }
            RetryFailure::CallbackFailed { callback, .. } => {
                let message = format!(
                    "HTTP retry callback failed after {attempts} attempt(s): {callback}"
                );
                return HttpError::other(message).with_source(error);
            }
            RetryFailure::Infrastructure { failure, .. } => {
                let message = format!(
                    "HTTP retry infrastructure failed after {attempts} attempt(s): {failure}"
                );
                return HttpError::other(message).with_source(error);
            }
            RetryFailure::Aborted { .. } | RetryFailure::Exhausted { .. } => {}
            _ => {
                return HttpError::other(format!(
                    "HTTP retry stopped after {attempts} attempt(s): {}",
                    error.failure(),
                ))
                .with_source(error);
            }
        }

        let (failure, _) = error.into_parts();
        match failure {
            RetryFailure::Aborted { last_failure, .. } => {
                let error = last_failure.into_error().expect(
                    "HTTP retry abort should preserve its application error",
                );
                if error.kind == crate::HttpErrorKind::Cancelled {
                    error
                } else {
                    Self::map_retry_aborted(error, attempts, started_at)
                }
            }
            RetryFailure::Exhausted {
                limit,
                last_failure,
                ..
            } => {
                let last_error =
                    last_failure.and_then(AttemptFailure::into_error);
                match limit {
                    RetryLimitKind::Attempts => {
                        let error = last_error.expect(
                            "HTTP attempt exhaustion should preserve its application error",
                        );
                        Self::map_retry_attempts_exhausted(
                            error,
                            attempts,
                            max_attempts,
                        )
                    }
                    RetryLimitKind::OperationElapsed
                    | RetryLimitKind::TotalElapsed => {
                        let max_duration = max_duration
                            .expect("HTTP elapsed limit requires max_duration");
                        Self::map_retry_max_duration_exceeded(
                            started_at,
                            max_duration,
                            last_error,
                        )
                    }
                }
            }
            _ => unreachable!("non-business retry terminals return above"),
        }
    }

    /// Builds a cancellation error for retry wait cancellation.
    ///
    /// # Parameters
    /// - `message`: Human-readable cancellation reason.
    /// - `method`: Request method to attach.
    /// - `url`: Optional resolved request URL to attach.
    ///
    /// # Returns
    /// [`HttpErrorKind::Cancelled`](crate::HttpErrorKind::Cancelled) with
    /// request context.
    fn retry_cancelled_error(
        message: &str,
        method: &http::Method,
        url: Option<&url::Url>,
    ) -> HttpError {
        let mut error = HttpError::cancelled(message).with_method(method);
        if let Some(url) = url {
            error = error.with_url(url);
        }
        error
    }

    /// Opens an SSE stream and reconnects automatically on retryable stream
    /// failures.
    ///
    /// Reconnect behavior:
    /// - retryable transport/read failures trigger reconnects;
    /// - optional reconnect on clean EOF (`reconnect_on_eof`);
    /// - `Last-Event-ID` is set from the latest parsed SSE last-event-id state;
    /// - optional use of SSE `retry:` as next reconnect delay.
    ///
    /// # Parameters
    /// - `request`: SSE request template reused on reconnect.
    /// - `options`: Reconnect limits and delay policy.
    ///
    /// # Returns
    /// SSE message stream yielding messages from one or more reconnect
    /// sessions.
    ///
    /// # Errors
    /// Stream items are `Result`; `Err` covers per-item failures such as:
    /// - initial stream-open failures when not reconnectable or retries
    ///   exhausted;
    /// - SSE protocol errors (non-reconnectable by default);
    /// - transport/read errors after reconnect budget is exhausted.
    ///
    /// # Side effects
    /// Performs repeated HTTP requests and reads on reconnect; may sleep
    /// between attempts according to reconnect options.
    pub fn execute_sse_with_reconnect(
        &self,
        request: HttpRequest,
        options: SseReconnectOptions,
    ) -> SseMessageStream {
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
