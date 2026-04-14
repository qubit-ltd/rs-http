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

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use async_stream::stream;
use bytes::Bytes;
use futures_util::StreamExt;
use http::header::RETRY_AFTER;
use http::{HeaderMap, StatusCode};
use httpdate::parse_http_date;
use qubit_function::MutatingFunction;
use qubit_retry::{
    AttemptFailure, Jitter, RetryDecision, RetryError, RetryExecutor, RetryOptions, RetryResult,
};
use reqwest::Response;
use tokio_util::sync::CancellationToken;
use url::Host;
use url::Url;

use crate::{
    AsyncHeaderInjector, HeaderInjector, HttpClientOptions, HttpError, HttpErrorKind, HttpLogger,
    HttpRequest, HttpRequestBody, HttpRequestBuilder, HttpResponse, HttpResult, HttpRetryOptions,
    HttpStreamResponse, RetryHint,
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
    /// Async header injectors applied after sync injectors and before request-level headers.
    async_injectors: Vec<AsyncHeaderInjector>,
}

/// Shared pre-send outcome for one HTTP attempt.
struct PreparedRequestSend {
    /// Request method used for this attempt.
    method: http::Method,
    /// Resolved request URL used for this attempt.
    url: Url,
    /// Optional cancellation token bound to this request.
    cancellation_token: Option<CancellationToken>,
    /// Raw response returned by reqwest for this attempt.
    response: Response,
}

/// Shared state used to carry extra `Retry-After` delay into the next async
/// retry attempt.
type PendingRetryAfterDelay = Arc<Mutex<Option<Duration>>>;

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
            .field("async_injectors", &self.async_injectors)
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
            async_injectors: Vec::new(),
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
        let retry_options = self.resolve_retry_options(&request);
        let honor_retry_after = request.retry_override.should_honor_retry_after();
        if !self.should_retry_request(&request, &retry_options) {
            return self.execute_once(request).await;
        }
        self.execute_with_retry(request, retry_options, honor_retry_after)
            .await
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
        let PreparedRequestSend {
            method,
            url,
            cancellation_token,
            response,
        } = self
            .prepare_and_send_once(request, "Request cancelled before sending")
            .await?;

        let response = self
            .ensure_success_response(response, &method, &url, "HTTP request failed")
            .await?;

        let status = response.status();
        let response_url = response.url().clone();
        let response_headers = response.headers().clone();

        let body = self
            .read_body_with_timeout(
                response,
                method.clone(),
                response_url.clone(),
                cancellation_token.as_ref(),
            )
            .await?;

        let logger = HttpLogger::new(&self.options.logging, &self.options.sensitive_headers);
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
        let retry_options = self.resolve_retry_options(&request);
        let honor_retry_after = request.retry_override.should_honor_retry_after();
        if !self.should_retry_request(&request, &retry_options) {
            return self.execute_stream_once(request).await;
        }
        self.execute_stream_with_retry(request, retry_options, honor_retry_after)
            .await
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
        let PreparedRequestSend {
            method,
            url,
            cancellation_token,
            response,
        } = self
            .prepare_and_send_once(request, "Streaming request cancelled before sending")
            .await?;

        let response = self
            .ensure_success_response(
                response,
                &method,
                &url,
                "HTTP streaming request failed",
            )
            .await?;

        let status = response.status();
        let response_url = response.url().clone();
        let response_headers = response.headers().clone();

        let logger = HttpLogger::new(&self.options.logging, &self.options.sensitive_headers);
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

    /// Resolves URL, applies headers/query/body/timeout, logs request, then sends one attempt.
    ///
    /// # Parameters
    /// - `request`: Request to execute.
    /// - `cancellation_message`: Error message used when cancelled before send.
    ///
    /// # Returns
    /// Request context and raw response for this attempt.
    async fn prepare_and_send_once(
        &self,
        request: HttpRequest,
        cancellation_message: &str,
    ) -> HttpResult<PreparedRequestSend> {
        let url = self.resolve_url(&request)?;
        let method = request.method.clone();
        let cancellation_token = request.cancellation_token.clone();
        if let Some(error) = cancelled_request_error_if_needed(
            cancellation_token.as_ref(),
            &method,
            &url,
            cancellation_message,
        ) {
            return Err(error);
        }
        let headers = self.build_headers(&request).await?;
        let body_for_log = clone_request_body_for_log(&request.body);

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
        builder = apply_request_body(builder, request.body);

        let response = self
            .send_with_write_timeout(
                builder,
                method.clone(),
                url.clone(),
                cancellation_token.as_ref(),
            )
            .await?;
        Ok(PreparedRequestSend {
            method,
            url,
            cancellation_token,
            response,
        })
    }

    /// Converts a non-success response into [`HttpError`] with status/retry/body-preview context.
    ///
    /// # Parameters
    /// - `response`: Raw response from reqwest.
    /// - `method`: Request method used for this attempt.
    /// - `url`: Resolved request URL used for this attempt.
    /// - `message_prefix`: Prefix for the final error message.
    ///
    /// # Returns
    /// Original response when successful, otherwise mapped [`HttpError`].
    async fn ensure_success_response(
        &self,
        response: Response,
        method: &http::Method,
        url: &Url,
        message_prefix: &str,
    ) -> HttpResult<Response> {
        if response.status().is_success() {
            return Ok(response);
        }

        let status = response.status();
        let retry_after = parse_retry_after(status, response.headers());
        let error = response.error_for_status_ref().expect_err(
            "non-success HTTP status must produce reqwest status error via error_for_status_ref",
        );
        let body_preview = self.read_error_response_preview(response).await;
        let message = format!(
            "{} with status {} for {} {}; response body preview: {}",
            message_prefix, status, method, url, body_preview
        );
        let mut mapped = map_reqwest_error(
            error,
            HttpErrorKind::Status,
            None,
            Some(method.clone()),
            Some(url.clone()),
        )
        .with_status(status)
        .with_response_body_preview(body_preview);
        if let Some(retry_after) = retry_after {
            mapped = mapped.with_retry_after(retry_after);
        }
        mapped.message = message;
        Err(mapped)
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
        retry_options.max_attempts > 1 && retry_options.allows_method(&request.method)
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
        options.enabled = request.retry_override.resolve_enabled(options.enabled);
        options.method_policy = request
            .retry_override
            .resolve_method_policy(options.method_policy);
        options
    }

    /// Builds a [`RetryExecutor`] from effective retry options and classifies
    /// [`HttpError`] values using [`RetryHint`].
    ///
    /// # Parameters
    /// - `retry_options`: Effective retry options for this request.
    /// - `honor_retry_after`: Whether to honor `Retry-After` on retryable
    ///   status responses (`429` and `5xx`).
    ///
    /// # Returns
    /// Configured executor plus optional shared extra-delay state, or
    /// [`HttpError`] if retry options or executor configuration is invalid.
    fn build_retry_executor(
        &self,
        retry_options: &HttpRetryOptions,
        honor_retry_after: bool,
    ) -> HttpResult<(RetryExecutor<HttpError>, Option<PendingRetryAfterDelay>)> {
        let options = RetryOptions::new(
            retry_options.max_attempts,
            retry_options.max_duration,
            retry_options.delay_strategy.clone(),
            Jitter::factor(retry_options.jitter_factor),
        )
        .map_err(|error| HttpError::other(format!("Invalid HTTP retry options: {error}")))?;

        let mut builder = RetryExecutor::<HttpError>::builder()
            .options(options)
            .classify_error(|error: &HttpError, _| {
                if matches!(error.retry_hint(), RetryHint::Retryable) {
                    RetryDecision::Retry
                } else {
                    RetryDecision::Abort
                }
            });
        if honor_retry_after {
            let pending_retry_after_delay: PendingRetryAfterDelay = Arc::new(Mutex::new(None));
            let pending_for_listener = Arc::clone(&pending_retry_after_delay);
            builder = builder.on_retry(move |context, failure| {
                let AttemptFailure::Error(error) = failure else {
                    return;
                };
                let Some(retry_after) = error.retry_after else {
                    return;
                };
                if retry_after > context.next_delay {
                    set_pending_retry_after_delay(
                        &pending_for_listener,
                        retry_after - context.next_delay,
                    );
                }
            });
            return builder
                .build()
                .map(|policy| (policy, Some(pending_retry_after_delay)))
                .map_err(|error| {
                    HttpError::other(format!("Invalid HTTP retry executor: {error}"))
                });
        }
        builder
            .build()
            .map(|policy| (policy, None))
            .map_err(|error| HttpError::other(format!("Invalid HTTP retry executor: {error}")))
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
        let (policy, pending_retry_after_delay) =
            self.build_retry_executor(&retry_options, honor_retry_after)?;
        let client = self.clone();
        let result = policy
            .run_async(move || {
                let client = client.clone();
                let request = request.clone();
                let pending_retry_after_delay = pending_retry_after_delay.clone();
                async move {
                    if let Some(delay) = pending_retry_after_delay
                        .as_ref()
                        .and_then(take_pending_retry_after_delay)
                    {
                        tokio::time::sleep(delay).await;
                    }
                    client.execute_once(request).await
                }
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
        let (policy, pending_retry_after_delay) =
            self.build_retry_executor(&retry_options, honor_retry_after)?;
        let client = self.clone();
        let result = policy
            .run_async(move || {
                let client = client.clone();
                let request = request.clone();
                let pending_retry_after_delay = pending_retry_after_delay.clone();
                async move {
                    if let Some(delay) = pending_retry_after_delay
                        .as_ref()
                        .and_then(take_pending_retry_after_delay)
                    {
                        tokio::time::sleep(delay).await;
                    }
                    client.execute_stream_once(request).await
                }
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
            self.validate_resolved_url_host(&url)?;
            return Ok(url);
        }

        let base = self.options.base_url.as_ref().ok_or_else(|| {
            HttpError::invalid_url(format!(
                "Cannot resolve relative path '{}' without base_url",
                request.path
            ))
        })?;

        let url = base.join(&request.path).map_err(|error| {
            HttpError::invalid_url(format!(
                "Failed to resolve path '{}' against base URL '{}': {}",
                request.path, base, error
            ))
        })?;
        self.validate_resolved_url_host(&url)?;
        Ok(url)
    }

    /// Validates host constraints for a resolved URL under current client options.
    ///
    /// # Parameters
    /// - `url`: Fully resolved request URL.
    ///
    /// # Returns
    /// `Ok(())` when host is allowed by options.
    ///
    /// # Errors
    /// Returns [`HttpError::invalid_url`] when `ipv4_only=true` and `url` uses an IPv6 literal host.
    fn validate_resolved_url_host(&self, url: &Url) -> HttpResult<()> {
        if self.options.ipv4_only && matches!(url.host(), Some(Host::Ipv6(_))) {
            return Err(HttpError::invalid_url(format!(
                "IPv6 literal host is not allowed when ipv4_only=true: {}",
                url
            )));
        }
        Ok(())
    }

    /// Merges default headers, injector output, and per-request headers (later
    /// wins on duplicates).
    ///
    /// # Parameters
    /// - `request`: Request supplying extra headers.
    ///
    /// # Returns
    /// Final [`HeaderMap`] or error if an injector fails.
    async fn build_headers(&self, request: &HttpRequest) -> HttpResult<HeaderMap> {
        let mut headers = self.options.default_headers.clone();

        for injector in &self.injectors {
            injector.apply(&mut headers)?;
        }
        for injector in &self.async_injectors {
            injector.apply(&mut headers).await?;
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
        cancellation_token: Option<&CancellationToken>,
    ) -> HttpResult<Response> {
        let timeout = self.options.timeouts.write_timeout;
        let send_future = tokio::time::timeout(timeout, builder.send());
        let next = if let Some(token) = cancellation_token {
            tokio::select! {
                _ = token.cancelled() => {
                    return Err(HttpError::cancelled("Request cancelled while sending")
                        .with_method(method)
                        .with_url(url));
                }
                send_result = send_future => send_result,
            }
        } else {
            send_future.await
        };
        match next {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(error)) => Err(map_reqwest_error(
                error,
                HttpErrorKind::Transport,
                Some(ReqwestErrorPhase::Send),
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
        cancellation_token: Option<&CancellationToken>,
    ) -> HttpResult<Bytes> {
        let timeout = self.options.timeouts.read_timeout;
        let read_future = tokio::time::timeout(timeout, response.bytes());
        let next = if let Some(token) = cancellation_token {
            tokio::select! {
                _ = token.cancelled() => {
                    return Err(HttpError::cancelled("Request cancelled while reading response body")
                        .with_method(method)
                        .with_url(url));
                }
                read_result = read_future => read_result,
            }
        } else {
            read_future.await
        };
        match next {
            Ok(Ok(body)) => Ok(body),
            Ok(Err(error)) => Err(map_reqwest_error(
                error,
                HttpErrorKind::Decode,
                Some(ReqwestErrorPhase::Read),
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

    /// Reads and renders a bounded preview for a non-success response body.
    ///
    /// # Parameters
    /// - `response`: Non-success response whose body will be consumed.
    ///
    /// # Returns
    /// Rendered preview text. On preview read failure, returns a descriptive placeholder.
    async fn read_error_response_preview(&self, mut response: Response) -> String {
        let read_timeout = self.options.timeouts.read_timeout;
        let max_bytes = self.options.error_response_preview_limit.max(1);
        let mut preview = Vec::new();
        let mut truncated = false;

        loop {
            let next = tokio::time::timeout(read_timeout, response.chunk()).await;
            match next {
                Ok(Ok(Some(chunk))) => {
                    if preview.len() >= max_bytes {
                        truncated = true;
                        break;
                    }
                    let remaining = max_bytes - preview.len();
                    if chunk.len() > remaining {
                        preview.extend_from_slice(&chunk[..remaining]);
                        truncated = true;
                        break;
                    }
                    preview.extend_from_slice(&chunk);
                }
                Ok(Ok(None)) => break,
                Ok(Err(error)) => {
                    return format!(
                        "<error body unavailable: failed to read response body: {}>",
                        error
                    );
                }
                Err(_) => {
                    return format!(
                        "<error body unavailable: read timeout after {:?}>",
                        read_timeout
                    );
                }
            }
        }

        render_error_body_preview(&preview, truncated)
    }
}

/// Builds a cancelled error when `token` is already cancelled.
///
/// # Parameters
/// - `token`: Optional cancellation token for this request.
/// - `method`: Request method for error context.
/// - `url`: Request URL for error context.
/// - `message`: Cancellation message.
///
/// # Returns
/// `Some(HttpError)` when cancelled, otherwise `None`.
fn cancelled_request_error_if_needed(
    token: Option<&CancellationToken>,
    method: &http::Method,
    url: &Url,
    message: &str,
) -> Option<HttpError> {
    if token.is_some_and(CancellationToken::is_cancelled) {
        Some(
            HttpError::cancelled(message.to_string())
                .with_method(method.clone())
                .with_url(url.clone()),
        )
    } else {
        None
    }
}

/// Clones request body content for request logging.
///
/// # Parameters
/// - `body`: Request body variant.
///
/// # Returns
/// Optional byte payload for logger previewing.
fn clone_request_body_for_log(body: &HttpRequestBody) -> Option<Bytes> {
    match body {
        HttpRequestBody::Bytes(bytes)
        | HttpRequestBody::Json(bytes)
        | HttpRequestBody::Form(bytes)
        | HttpRequestBody::Multipart(bytes)
        | HttpRequestBody::Ndjson(bytes) => Some(bytes.clone()),
        HttpRequestBody::Text(text) => Some(Bytes::from(text.clone())),
        HttpRequestBody::Empty => None,
    }
}

/// Applies request body variant to a reqwest request builder.
///
/// # Parameters
/// - `builder`: Request builder with method/url/headers/query already set.
/// - `body`: Request body variant to apply.
///
/// # Returns
/// Updated builder containing the request body payload.
fn apply_request_body(
    builder: reqwest::RequestBuilder,
    body: HttpRequestBody,
) -> reqwest::RequestBuilder {
    match body {
        HttpRequestBody::Empty => builder,
        HttpRequestBody::Bytes(bytes)
        | HttpRequestBody::Json(bytes)
        | HttpRequestBody::Form(bytes)
        | HttpRequestBody::Multipart(bytes)
        | HttpRequestBody::Ndjson(bytes) => builder.body(bytes),
        HttpRequestBody::Text(text) => builder.body(text),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReqwestErrorPhase {
    /// Error happened while sending request / waiting first response bytes.
    Send,
    /// Error happened while reading response body.
    Read,
}

/// Maps a [`reqwest::Error`] into [`HttpError`] with phase-aware timeout
/// classification and optional context.
///
/// # Parameters
/// - `error`: Underlying reqwest error.
/// - `default_kind`: Kind used when reqwest does not classify the error more
///   specifically.
/// - `phase`: Optional execution phase used to classify timeout errors.
/// - `method`: Optional request method to attach.
/// - `url`: Optional request URL to attach.
///
/// # Returns
/// Configured [`HttpError`] including chained source.
fn map_reqwest_error(
    error: reqwest::Error,
    default_kind: HttpErrorKind,
    phase: Option<ReqwestErrorPhase>,
    method: Option<http::Method>,
    url: Option<Url>,
) -> HttpError {
    let kind = if error.is_timeout() {
        classify_reqwest_timeout_kind(&error, phase)
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

/// Classifies reqwest timeout errors by execution phase.
///
/// # Parameters
/// - `error`: Reqwest timeout error to classify.
/// - `phase`: Optional phase where timeout happened.
///
/// # Returns
/// Timeout kind inferred from phase:
/// - send phase: `ConnectTimeout` when reqwest marks connect errors; otherwise `RequestTimeout`;
/// - read phase: `ReadTimeout`;
/// - unknown phase: `RequestTimeout`.
fn classify_reqwest_timeout_kind(
    error: &reqwest::Error,
    phase: Option<ReqwestErrorPhase>,
) -> HttpErrorKind {
    match phase {
        Some(ReqwestErrorPhase::Send) => {
            if error.is_connect() {
                HttpErrorKind::ConnectTimeout
            } else {
                HttpErrorKind::RequestTimeout
            }
        }
        Some(ReqwestErrorPhase::Read) => HttpErrorKind::ReadTimeout,
        None => HttpErrorKind::RequestTimeout,
    }
}

/// Parses `Retry-After` from response headers when status is retryable.
///
/// # Parameters
/// - `status`: HTTP status code.
/// - `headers`: Response headers.
///
/// # Returns
/// Parsed retry delay when `status` is `429` or `5xx` and `Retry-After` is
/// present in `delta-seconds` or HTTP-date format.
fn parse_retry_after(status: StatusCode, headers: &HeaderMap) -> Option<Duration> {
    if !is_retry_after_applicable_status(status) {
        return None;
    }
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_retry_after_value)
}

/// Returns whether a status code should honor `Retry-After`.
///
/// # Parameters
/// - `status`: HTTP status code.
///
/// # Returns
/// `true` for `429` and `5xx` statuses; otherwise `false`.
fn is_retry_after_applicable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// Parses a `Retry-After` header value as delta-seconds or HTTP-date.
///
/// # Parameters
/// - `value`: Raw `Retry-After` header value.
///
/// # Returns
/// Parsed duration, or `None` when value is neither valid delta-seconds nor a
/// valid HTTP-date.
fn parse_retry_after_value(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(seconds) = trimmed.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let retry_at = parse_http_date(trimmed).ok()?;
    let now = SystemTime::now();
    Some(
        retry_at
            .duration_since(now)
            .unwrap_or_else(|_| Duration::from_secs(0)),
    )
}

/// Stores the extra `Retry-After` delay that should be applied before the next
/// retry attempt.
///
/// # Parameters
/// - `pending`: Shared state carrying a pending delay.
/// - `delay`: Extra delay to store.
fn set_pending_retry_after_delay(pending: &PendingRetryAfterDelay, delay: Duration) {
    let mut guard = pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(delay);
}

/// Takes and clears the pending extra `Retry-After` delay.
///
/// # Parameters
/// - `pending`: Shared state carrying a pending delay.
///
/// # Returns
/// Pending delay if one exists.
fn take_pending_retry_after_delay(pending: &PendingRetryAfterDelay) -> Option<Duration> {
    let mut guard = pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.take()
}

/// Renders a human-readable error-body preview from raw bytes.
///
/// # Parameters
/// - `bytes`: Captured body bytes (already size-limited).
/// - `truncated`: Whether additional bytes were omitted.
///
/// # Returns
/// UTF-8 text preview or binary placeholder with truncation suffix when needed.
fn render_error_body_preview(bytes: &[u8], truncated: bool) -> String {
    if bytes.is_empty() {
        return "<empty>".to_string();
    }

    let suffix = if truncated { "...<truncated>" } else { "" };
    match std::str::from_utf8(bytes) {
        Ok(text) => format!("{text}{suffix}"),
        Err(_) => format!("<binary {} bytes>{suffix}", bytes.len()),
    }
}
