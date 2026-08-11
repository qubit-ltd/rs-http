// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! [`SseReconnectRunner`] implementation used by
//! [`HttpClient`](crate::HttpClient).

use std::error::Error as StdError;
use std::io::ErrorKind;
use std::time::Duration;

use async_stream::stream;
use futures_util::StreamExt;
use http::header::CONTENT_TYPE;
use http::header::HeaderName;
use http::header::HeaderValue;
use qubit_clock::StdMonotonicClock;
use qubit_redact::http::HttpRedactor;
use qubit_retry::BackoffRequest;
use qubit_retry::BackoffState;
use qubit_retry::RetryBudget;
use qubit_retry::RetryBudgetExhausted;
use qubit_retry::RetryPolicy;
use tokio_util::sync::CancellationToken;

use super::DEFAULT_SSE_MAX_RECONNECT_DELAY;
use super::SseControl;
use super::SseMessageStream;
use super::SseReconnectOptions;
use super::SseRecord;
use crate::HttpClient;
use crate::HttpError;
use crate::HttpErrorKind;
use crate::HttpRequest;
use crate::HttpResponse;
use crate::HttpResult;
use crate::RetryHint;
use crate::content_type;

/// Header name used for SSE resume token propagation.
const LAST_EVENT_ID_HEADER: &str = "last-event-id";

/// Runtime references shared by one reconnect scheduling decision.
struct ReconnectRuntime<'a> {
    /// Retry policy used to derive delay and jitter behavior.
    retry_policy: &'a RetryPolicy,
    /// SSE reconnect options controlling server retry and EOF behavior.
    options: &'a SseReconnectOptions,
    /// Optional cancellation token checked while sleeping before reconnect.
    cancellation_token: Option<&'a CancellationToken>,
    /// Request method used in reconnect cancellation and max-elapsed errors.
    request_method: &'a http::Method,
    /// Request URL used in reconnect cancellation and max-elapsed errors.
    request_url: Option<&'a url::Url>,
    /// Shared request redactor used by reconnect-generated errors.
    log_redactor: &'a HttpRedactor,
}

/// Outcome after trying to schedule one reconnect.
enum ReconnectAction {
    /// Reconnect sleep completed and caller should start the next attempt.
    Continue,
    /// Reconnect is exhausted without an error to yield.
    Stop,
    /// Reconnect failed and caller should yield this error.
    Fail(Box<HttpError>),
}

/// Mutable reconnect backoff and pending retry delay state.
struct ReconnectState {
    /// Current local backoff delay before jitter/server override.
    backoff: BackoffState,
    /// Optional one-shot server-provided retry delay from an SSE `retry:`
    /// record.
    pending_server_retry_delay: Option<Duration>,
}

impl ReconnectState {
    /// Creates reconnect state from retry policy backoff configuration.
    ///
    /// # Parameters
    /// - `retry_policy`: Retry policy used to derive the initial delay.
    ///
    /// # Returns
    /// New reconnect state with no pending server-provided delay.
    fn new(retry_policy: &RetryPolicy) -> Self {
        Self {
            backoff: retry_policy.backoff().start(),
            pending_server_retry_delay: None,
        }
    }

    /// Stores one server-provided retry delay for the next reconnect only.
    ///
    /// # Parameters
    /// - `delay`: Delay derived from an SSE `retry:` record.
    ///
    /// # Returns
    /// Nothing.
    fn set_server_retry_delay(&mut self, delay: Duration) {
        self.pending_server_retry_delay = Some(delay);
    }

    /// Attempts to schedule a reconnect after a retryable error.
    ///
    /// # Parameters
    /// - `error`: Last retryable HTTP/SSE error.
    /// - `runtime`: Shared reconnect policy and diagnostic context.
    ///
    /// # Returns
    /// Reconnect action for the caller.
    ///
    /// # Side effects
    /// Sleeps asynchronously when another reconnect is allowed.
    async fn after_error(
        &mut self,
        error: HttpError,
        budget: &RetryBudget<'_>,
        runtime: &ReconnectRuntime<'_>,
    ) -> ReconnectAction {
        let sleep_delay = self.sleep_delay(runtime);
        match budget.check_retry_after(sleep_delay) {
            Ok(()) => self.sleep_and_advance(sleep_delay, runtime).await,
            Err(RetryBudgetExhausted::Attempts) => ReconnectAction::Fail(Box::new(error)),
            Err(RetryBudgetExhausted::OperationElapsed) => {
                ReconnectAction::Fail(Box::new(operation_elapsed_exceeded_error_with_last_error(
                    error,
                    budget.snapshot().operation_elapsed(),
                    runtime.retry_policy.limits().max_operation_elapsed(),
                    runtime.request_method,
                    runtime.request_url,
                    runtime.log_redactor,
                )))
            }
            Err(RetryBudgetExhausted::TotalElapsed) => {
                ReconnectAction::Fail(Box::new(max_elapsed_exceeded_error_with_last_error(
                    error,
                    budget.snapshot().total_elapsed(),
                    runtime
                        .retry_policy
                        .limits()
                        .max_total_elapsed()
                        .expect("total budget exhaustion requires a configured total limit"),
                    runtime.request_method,
                    runtime.request_url,
                    runtime.log_redactor,
                )))
            }
        }
    }

    /// Attempts to schedule a reconnect after a clean stream EOF.
    ///
    /// # Parameters
    /// - `runtime`: Shared reconnect policy and diagnostic context.
    ///
    /// # Returns
    /// Reconnect action for the caller.
    ///
    /// # Side effects
    /// Sleeps asynchronously when another reconnect is allowed.
    async fn after_eof(
        &mut self,
        budget: &RetryBudget<'_>,
        runtime: &ReconnectRuntime<'_>,
    ) -> ReconnectAction {
        let sleep_delay = self.sleep_delay(runtime);
        match budget.check_retry_after(sleep_delay) {
            Ok(()) => self.sleep_and_advance(sleep_delay, runtime).await,
            Err(RetryBudgetExhausted::Attempts) => ReconnectAction::Stop,
            Err(RetryBudgetExhausted::OperationElapsed) => {
                ReconnectAction::Fail(Box::new(operation_elapsed_exceeded_error(
                    budget.snapshot().operation_elapsed(),
                    runtime.retry_policy.limits().max_operation_elapsed(),
                    runtime.request_method,
                    runtime.request_url,
                    runtime.log_redactor,
                )))
            }
            Err(RetryBudgetExhausted::TotalElapsed) => {
                ReconnectAction::Fail(Box::new(max_elapsed_exceeded_error(
                    budget.snapshot().total_elapsed(),
                    runtime
                        .retry_policy
                        .limits()
                        .max_total_elapsed()
                        .expect("total budget exhaustion requires a configured total limit"),
                    runtime.request_method,
                    runtime.request_url,
                    runtime.log_redactor,
                )))
            }
        }
    }

    /// Returns the effective sleep delay for the next reconnect.
    ///
    /// # Parameters
    /// - `runtime`: Shared reconnect policy context.
    ///
    /// # Returns
    /// Jittered or server-provided reconnect delay.
    fn sleep_delay(&mut self, runtime: &ReconnectRuntime<'_>) -> Duration {
        let request = match self.pending_server_retry_delay {
            Some(delay) if runtime.options.apply_jitter_to_server_retry => {
                BackoffRequest::jittered_hint(delay)
            }
            Some(delay) => BackoffRequest::hint(delay),
            None => BackoffRequest::policy(),
        };
        self.backoff
            .next(request)
            .effective_delay()
            .max(Duration::from_millis(1))
    }

    /// Sleeps before reconnect and clears one-shot reconnect state.
    ///
    /// # Parameters
    /// - `sleep_delay`: Effective reconnect delay to wait.
    /// - `runtime`: Shared cancellation and policy context.
    ///
    /// # Returns
    /// [`ReconnectAction::Continue`] after a successful sleep, or
    /// [`ReconnectAction::Fail`] when cancellation interrupts the sleep.
    ///
    /// # Side effects
    /// Sleeps asynchronously and clears the one-shot server retry delay.
    async fn sleep_and_advance(
        &mut self,
        sleep_delay: Duration,
        runtime: &ReconnectRuntime<'_>,
    ) -> ReconnectAction {
        if let Err(error) = sleep_reconnect_delay(
            sleep_delay,
            runtime.cancellation_token,
            runtime.request_method,
            runtime.request_url,
            runtime.log_redactor,
        )
        .await
        {
            return ReconnectAction::Fail(Box::new(error));
        }
        self.pending_server_retry_delay = None;
        ReconnectAction::Continue
    }
}

/// Stateful SSE reconnect runner for one stream invocation.
pub(crate) struct SseReconnectRunner {
    /// HTTP client used to execute each stream attempt.
    client: HttpClient,
    /// Request template cloned for every reconnect attempt; a `Last-Event-ID`
    /// header may be applied on resume.
    request_template: HttpRequest,
    /// Reconnect limits, backoff, and stream behavior flags.
    options: SseReconnectOptions,
}

impl SseReconnectRunner {
    /// Creates a reconnect runner bound to one outbound request and options.
    ///
    /// # Parameters
    /// - `client`: HTTP client used to open each stream attempt.
    /// - `request`: Request reused (cloned) across reconnect attempts.
    /// - `options`: Reconnect limits and delay policy.
    ///
    /// # Returns
    /// New SSE reconnect runner.
    pub(crate) fn new(
        client: HttpClient,
        request: HttpRequest,
        options: SseReconnectOptions,
    ) -> Self {
        Self {
            client,
            request_template: request,
            options,
        }
    }

    /// Starts the reconnect loop and returns a merged SSE message stream.
    ///
    /// # Returns
    /// SSE message stream yielding messages from one or more reconnect
    /// sessions.
    pub(crate) fn run(self) -> SseMessageStream {
        let client = self.client;
        let request_template = self.request_template;
        let options = self.options;
        let output = stream! {
            let retry_policy = options.retry.clone();
            let request_url = request_template.resolved_url().ok();
            let request_method = request_template.method().clone();
            let cancellation_token = request_template.cancellation_token().cloned();
            let log_redactor = request_template.log_redactor().clone();
            let runtime = ReconnectRuntime {
                retry_policy: &retry_policy,
                options: &options,
                cancellation_token: cancellation_token.as_ref(),
                request_method: &request_method,
                request_url: request_url.as_ref(),
                log_redactor: &log_redactor,
            };
            let mut reconnect_state = ReconnectState::new(&retry_policy);
            let clock = StdMonotonicClock::new();
            let mut retry_budget = match RetryBudget::new(&clock, *retry_policy.limits()) {
                Ok(budget) => budget,
                Err(error) => {
                    let mut mapped = HttpError::other(format!(
                        "Failed to create SSE reconnect budget: {error}",
                    ))
                    .with_method(&request_method)
                    .with_log_redactor(log_redactor.clone());
                    if let Some(url) = request_url.as_ref() {
                        mapped = mapped.with_url(url);
                    }
                    yield Err(mapped);
                    return;
                }
            };
            let mut last_event_id: Option<String> = None;
            loop {
                let attempt = match retry_budget.begin_attempt() {
                    Ok(attempt) => attempt,
                    Err(RetryBudgetExhausted::Attempts) => return,
                    Err(RetryBudgetExhausted::OperationElapsed) => {
                        yield Err(operation_elapsed_exceeded_error(
                            retry_budget.snapshot().operation_elapsed(),
                            retry_policy.limits().max_operation_elapsed(),
                            runtime.request_method,
                            runtime.request_url,
                            runtime.log_redactor,
                        ));
                        return;
                    }
                    Err(RetryBudgetExhausted::TotalElapsed) => {
                        yield Err(max_elapsed_exceeded_error(
                            retry_budget.snapshot().total_elapsed(),
                            retry_policy.limits().max_total_elapsed().expect(
                                "total budget exhaustion requires a configured total limit",
                            ),
                            runtime.request_method,
                            runtime.request_url,
                            runtime.log_redactor,
                        ));
                        return;
                    }
                };
                let mut request = request_template.clone();
                // SSE reconnect loop already retries at stream level. Disable
                // inner HTTP retry to avoid multiplicative retry attempts.
                let retry_override = request.retry_override().clone().force_disable();
                request.set_retry_override(retry_override);
                if let Some(last_event_id) = last_event_id.as_deref() {
                    if let Err(error) = apply_last_event_id_header(
                        &mut request,
                        last_event_id,
                        runtime.log_redactor,
                    ) {
                        let _ = retry_budget.finish_attempt(attempt);
                        yield Err(error);
                        return;
                    }
                }

                let response = match client.execute_once(request).await {
                    Ok(response) => response,
                    Err(error) => {
                        let _ = retry_budget.finish_attempt(attempt);
                        if should_reconnect_sse_error(&error) {
                            match reconnect_state.after_error(error, &retry_budget, &runtime).await {
                                ReconnectAction::Continue => continue,
                                ReconnectAction::Stop => return,
                                ReconnectAction::Fail(error) => {
                                    yield Err(*error);
                                    return;
                                }
                            }
                        }
                        yield Err(error);
                        return;
                    }
                };
                if let Err(error) =
                    validate_sse_response_content_type(&response, runtime.log_redactor)
                {
                    let _ = retry_budget.finish_attempt(attempt);
                    yield Err(error);
                    return;
                }
                let _ = retry_budget.finish_attempt(attempt);

                let mut records = response.sse_records();
                let mut stream_error: Option<HttpError> = None;
                while let Some(item) = records.next().await {
                    match item {
                        Ok(SseRecord::Dispatch(message)) => {
                            if let Some(id) = message.last_event_id.clone() {
                                last_event_id = Some(id);
                            }
                            yield Ok(message);
                        }
                        Ok(SseRecord::Control(SseControl::ReconnectDelayMs(retry_ms))) => {
                            if options.honor_server_retry {
                                reconnect_state.set_server_retry_delay(server_retry_delay(
                                    retry_ms,
                                    &retry_policy,
                                    &options,
                                ));
                            }
                        }
                        Ok(SseRecord::Control(SseControl::LastEventId(id))) => {
                            last_event_id = Some(id);
                        }
                        Err(error) => {
                            stream_error = Some(error);
                            break;
                        }
                    }
                }

                if let Some(error) = stream_error {
                    if should_reconnect_sse_error(&error) {
                        match reconnect_state.after_error(error, &retry_budget, &runtime).await {
                            ReconnectAction::Continue => continue,
                            ReconnectAction::Stop => return,
                            ReconnectAction::Fail(error) => {
                                yield Err(*error);
                                return;
                            }
                        }
                    }
                    yield Err(error);
                    return;
                }

                if options.reconnect_on_eof {
                    match reconnect_state.after_eof(&retry_budget, &runtime).await {
                        ReconnectAction::Continue => continue,
                        ReconnectAction::Stop => return,
                        ReconnectAction::Fail(error) => {
                            yield Err(*error);
                            return;
                        }
                    }
                }
                return;
            }
        };
        Box::pin(output)
    }
}

/// Applies `Last-Event-ID` request header for SSE reconnection.
///
/// # Parameters
/// - `request`: Outbound request to mutate.
/// - `last_event_id`: Last received SSE event identifier.
///
/// # Returns
/// `Ok(())` when header is applied.
///
/// # Errors
/// Returns [`HttpError`] when `last_event_id` cannot be represented as an HTTP
/// header value.
fn apply_last_event_id_header(
    request: &mut HttpRequest,
    last_event_id: &str,
    log_redactor: &HttpRedactor,
) -> HttpResult<()> {
    let header_value =
        HeaderValue::from_str(last_event_id).map_err(|error| {
            HttpError::other(format!(
                "Invalid Last-Event-ID header value ({} bytes): {error}",
                last_event_id.len()
            ))
            .with_log_redactor(log_redactor.clone())
        })?;
    request.set_typed_header(
        HeaderName::from_static(LAST_EVENT_ID_HEADER),
        header_value,
    );
    Ok(())
}

/// Returns whether an SSE stream error should trigger auto reconnect.
///
/// # Parameters
/// - `error`: Stream or transport error from SSE execution.
///
/// # Returns
/// `true` for retryable transport-like errors except explicit cancellation.
fn should_reconnect_sse_error(error: &HttpError) -> bool {
    if error.kind == HttpErrorKind::Cancelled {
        return false;
    }
    matches!(error.retry_hint(), RetryHint::Retryable)
        || is_unexpected_eof_error(error)
}

/// Sleeps before reconnect, while honoring cancellation token when provided.
///
/// # Parameters
/// - `delay`: Reconnect delay to wait.
/// - `cancellation_token`: Optional cancellation token.
/// - `request_method`: Request method for cancellation error context.
/// - `request_url`: Optional request URL for cancellation error context.
/// - `log_redactor`: Shared request redactor attached to cancellation errors.
///
/// # Returns
/// `Ok(())` after sleep completes.
///
/// # Errors
/// Returns [`HttpErrorKind::Cancelled`] if cancellation is triggered during the
/// reconnect sleep window.
async fn sleep_reconnect_delay(
    delay: Duration,
    cancellation_token: Option<&CancellationToken>,
    request_method: &http::Method,
    request_url: Option<&url::Url>,
    log_redactor: &HttpRedactor,
) -> HttpResult<()> {
    if let Some(token) = cancellation_token {
        tokio::select! {
            _ = token.cancelled() => {
                let mut error = HttpError::cancelled(
                    "SSE reconnect cancelled while waiting before next attempt",
                )
                .with_method(request_method);
                if let Some(url) = request_url {
                    error = error.with_url(url);
                }
                Err(error.with_log_redactor(log_redactor.clone()))
            }
            _ = tokio::time::sleep(delay) => Ok(()),
        }
    } else {
        tokio::time::sleep(delay).await;
        Ok(())
    }
}

/// Returns reconnect delay derived from one SSE `retry:` value.
///
/// # Parameters
/// - `retry_ms`: Milliseconds from SSE `retry:` field.
/// - `retry_options`: Retry options used for fallback cap derivation.
/// - `options`: SSE reconnect options with optional server-retry cap override.
///
/// # Returns
/// Capped reconnect delay from server retry value.
fn server_retry_delay(
    retry_ms: u64,
    retry_policy: &RetryPolicy,
    options: &SseReconnectOptions,
) -> Duration {
    let raw = Duration::from_millis(retry_ms.max(1));
    let cap = server_retry_max_delay(retry_policy, options);
    raw.min(cap).max(Duration::from_millis(1))
}

/// Returns max allowed delay for SSE `retry:` values.
///
/// # Parameters
/// - `retry_options`: Retry options used for derived cap.
/// - `options`: SSE reconnect options with optional explicit cap.
///
/// # Returns
/// Maximum server retry delay.
fn server_retry_max_delay(
    retry_policy: &RetryPolicy,
    options: &SseReconnectOptions,
) -> Duration {
    options
        .server_retry_max_delay
        .unwrap_or_else(|| default_server_retry_max_delay(retry_policy))
        .max(Duration::from_millis(1))
}

/// Returns fallback server-retry cap derived from retry delay strategy.
///
/// # Parameters
/// - `retry_options`: Retry options whose delay strategy is inspected.
///
/// # Returns
/// Fallback cap for server-provided `retry:` delay.
fn default_server_retry_max_delay(retry_policy: &RetryPolicy) -> Duration {
    match retry_policy.backoff().maximum_delay() {
        Some(maximum) if maximum > Duration::from_millis(1) => maximum,
        _ => DEFAULT_SSE_MAX_RECONNECT_DELAY,
    }
}

/// Builds one reconnect total-elapsed error for reconnect-on-EOF path.
///
/// # Parameters
/// - `elapsed`: Current elapsed reconnect duration.
/// - `max_elapsed`: Configured max elapsed reconnect duration.
/// - `request_method`: Request method for diagnostics.
/// - `request_url`: Optional request URL for diagnostics.
/// - `log_redactor`: Shared request redactor attached to the returned error.
///
/// # Returns
/// Reconnect max-elapsed error with method/url context when available.
fn max_elapsed_exceeded_error(
    elapsed: Duration,
    max_elapsed: Duration,
    request_method: &http::Method,
    request_url: Option<&url::Url>,
    log_redactor: &HttpRedactor,
) -> HttpError {
    let mut error = HttpError::retry_max_elapsed_exceeded(format!(
        "SSE reconnect max duration exceeded: {elapsed:?}/{max_elapsed:?}"
    ))
    .with_method(request_method);
    if let Some(url) = request_url {
        error = error.with_url(url);
    }
    error.with_log_redactor(log_redactor.clone())
}

/// Builds one reconnect max-elapsed error while preserving one original retry
/// error as source context.
///
/// # Parameters
/// - `last_error`: Last reconnect-triggering retryable error.
/// - `elapsed`: Current elapsed reconnect duration.
/// - `max_elapsed`: Configured max elapsed reconnect duration.
/// - `request_method`: Request method for diagnostics fallback.
/// - `request_url`: Optional request URL for diagnostics fallback.
/// - `log_redactor`: Shared request redactor attached to the returned error.
///
/// # Returns
/// Reconnect max-elapsed error with original error preserved in source chain.
fn max_elapsed_exceeded_error_with_last_error(
    last_error: HttpError,
    elapsed: Duration,
    max_elapsed: Duration,
    request_method: &http::Method,
    request_url: Option<&url::Url>,
    log_redactor: &HttpRedactor,
) -> HttpError {
    let error = max_elapsed_exceeded_error(
        elapsed,
        max_elapsed,
        request_method,
        request_url,
        log_redactor,
    );
    attach_last_retryable_error(error, last_error)
}

/// Builds one reconnect operation-elapsed error.
///
/// # Parameters
/// - `elapsed`: Current accumulated connection-attempt duration.
/// - `max_elapsed`: Configured operation duration limit.
/// - `request_method`: Request method for diagnostics.
/// - `request_url`: Optional request URL for diagnostics.
/// - `log_redactor`: Shared request redactor attached to the returned error.
///
/// # Returns
/// Reconnect operation-elapsed error with method/url context when available.
fn operation_elapsed_exceeded_error(
    elapsed: Duration,
    max_elapsed: Option<Duration>,
    request_method: &http::Method,
    request_url: Option<&url::Url>,
    log_redactor: &HttpRedactor,
) -> HttpError {
    let max_elapsed = max_elapsed.expect(
        "operation budget exhaustion requires a configured operation limit",
    );
    let mut error = HttpError::retry_max_elapsed_exceeded(format!(
        "SSE reconnect max operation duration exceeded: {elapsed:?}/{max_elapsed:?}"
    ))
    .with_method(request_method);
    if let Some(url) = request_url {
        error = error.with_url(url);
    }
    error.with_log_redactor(log_redactor.clone())
}

/// Builds one reconnect operation-elapsed error with the last retryable error.
///
/// # Parameters
/// - `last_error`: Last reconnect-triggering retryable error.
/// - `elapsed`: Current accumulated connection-attempt duration.
/// - `max_elapsed`: Configured operation duration limit.
/// - `request_method`: Request method for diagnostics fallback.
/// - `request_url`: Optional request URL for diagnostics fallback.
/// - `log_redactor`: Shared request redactor attached to the returned error.
///
/// # Returns
/// Reconnect operation-elapsed error with the original error as its source.
fn operation_elapsed_exceeded_error_with_last_error(
    last_error: HttpError,
    elapsed: Duration,
    max_elapsed: Option<Duration>,
    request_method: &http::Method,
    request_url: Option<&url::Url>,
    log_redactor: &HttpRedactor,
) -> HttpError {
    let error = operation_elapsed_exceeded_error(
        elapsed,
        max_elapsed,
        request_method,
        request_url,
        log_redactor,
    );
    attach_last_retryable_error(error, last_error)
}

/// Preserves one retryable error as diagnostic context on a budget failure.
///
/// # Parameters
/// - `error`: Budget error that receives request context and a source.
/// - `last_error`: Last retryable HTTP/SSE error.
///
/// # Returns
/// The updated budget error with the last error in its source chain.
fn attach_last_retryable_error(
    mut error: HttpError,
    last_error: HttpError,
) -> HttpError {
    if let Some(method) = last_error.method.as_ref() {
        error = error.with_method(method);
    }
    if let Some(url) = last_error.url.as_ref() {
        error = error.with_url(url);
    }
    if let Some(status) = last_error.status {
        error = error.with_status(status);
    }
    let mut message = format!(
        "{}; last retryable error: {}",
        error.message, last_error.message
    );
    if let Some(status) = last_error.status {
        message = format!("{message} (status: {status})");
    }
    error.message = message;
    error.source = Some(Box::new(last_error));
    error
}

/// Validates whether response content type is SSE media type.
///
/// # Parameters
/// - `response`: HTTP response to validate.
///
/// # Returns
/// `Ok(())` when content type is `text/event-stream`.
///
/// # Errors
/// Returns [`HttpErrorKind::SseProtocol`] when `Content-Type` is missing,
/// non-UTF8, or not SSE media type.
fn validate_sse_response_content_type(
    response: &HttpResponse,
    log_redactor: &HttpRedactor,
) -> HttpResult<()> {
    let method = response.meta().method().clone();
    let url = response.request_url().clone();
    let Some(value) = response.headers().get(CONTENT_TYPE) else {
        return Err(HttpError::sse_protocol(
            "Missing Content-Type header for SSE response",
        )
        .with_status(response.status())
        .with_method(&method)
        .with_url(&url)
        .with_log_redactor(log_redactor.clone()));
    };
    let content_type = value.to_str().map_err(|_| {
        HttpError::sse_protocol(
            "Invalid non-UTF8 Content-Type header for SSE response",
        )
        .with_status(response.status())
        .with_method(&method)
        .with_url(&url)
        .with_log_redactor(log_redactor.clone())
    })?;
    if content_type::is_sse(content_type) {
        return Ok(());
    }
    Err(HttpError::sse_protocol(format!(
        "Expected Content-Type 'text/event-stream' for SSE response, got '{content_type}'"
    ))
    .with_status(response.status())
    .with_method(&method)
    .with_url(&url)
    .with_log_redactor(log_redactor.clone()))
}

/// Returns whether an HTTP error represents an unexpected stream EOF that is
/// suitable for SSE reconnect.
///
/// # Parameters
/// - `error`: HTTP error to inspect.
///
/// # Returns
/// `true` when message/source indicates unexpected EOF during stream decoding.
fn is_unexpected_eof_error(error: &HttpError) -> bool {
    let contains_unexpected_eof =
        |text: &str| text.to_ascii_lowercase().contains("unexpected eof");
    if contains_unexpected_eof(&error.message) {
        return true;
    }
    error.source.as_ref().is_some_and(|source| {
        has_unexpected_eof_in_error_chain(source.as_ref())
            || contains_unexpected_eof(&source.to_string())
            || contains_unexpected_eof(&format!("{source:?}"))
    })
}

/// Returns whether any error in the source chain is an unexpected EOF.
///
/// # Parameters
/// - `error`: Root source error to inspect.
///
/// # Returns
/// `true` when chain contains `std::io::ErrorKind::UnexpectedEof`.
fn has_unexpected_eof_in_error_chain(error: &(dyn StdError + 'static)) -> bool {
    let mut current: Option<&(dyn StdError + 'static)> = Some(error);
    while let Some(item) = current {
        if item
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io_error| io_error.kind() == ErrorKind::UnexpectedEof)
        {
            return true;
        }
        current = item.source();
    }
    false
}
