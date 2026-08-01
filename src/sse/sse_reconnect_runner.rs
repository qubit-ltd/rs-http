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
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use async_stream::stream;
use futures_util::StreamExt;
use http::header::{
    HeaderName,
    HeaderValue,
    CONTENT_TYPE,
};
use qubit_redact::http::HttpRedactor;
use tokio_util::sync::CancellationToken;

use super::{
    SseControl,
    SseMessageStream,
    SseReconnectOptions,
    SseRecord,
    DEFAULT_SSE_MAX_RECONNECT_DELAY,
};
use crate::{
    content_type,
    HttpClient,
    HttpError,
    HttpErrorKind,
    HttpRequest,
    HttpResponse,
    HttpResult,
    RetryHint,
};
use qubit_retry::{
    RetryDelay,
    RetryOptions,
};

/// Header name used for SSE resume token propagation.
const LAST_EVENT_ID_HEADER: &str = "last-event-id";

/// Reconnect gate decision before scheduling one more reconnect attempt.
enum ReconnectDecision {
    /// Reconnect is allowed by both attempt count and elapsed-time budget.
    Allowed,
    /// Reconnect is blocked because maximum reconnect count is reached.
    MaxReconnectsReached,
    /// Reconnect is blocked because elapsed-time budget is exhausted.
    MaxElapsedExceeded {
        /// Monotonic elapsed time since runner start.
        elapsed: Duration,
        /// Configured maximum elapsed time.
        max_elapsed: Duration,
    },
}

/// Runtime references shared by one reconnect scheduling decision.
struct ReconnectRuntime<'a> {
    /// Retry options used to derive count, elapsed, delay, and jitter
    /// behavior.
    retry_options: &'a RetryOptions,
    /// SSE reconnect options controlling server retry and EOF behavior.
    options: &'a SseReconnectOptions,
    /// Maximum reconnect attempts allowed after the first stream attempt.
    max_reconnects: u32,
    /// Runner start time used for elapsed-budget checks.
    started_at: Instant,
    /// Optional cancellation token checked while sleeping before reconnect.
    cancellation_token: Option<&'a CancellationToken>,
    /// Request method used in reconnect cancellation and max-elapsed errors.
    request_method: &'a http::Method,
    /// Request URL used in reconnect cancellation and max-elapsed errors.
    request_url: Option<&'a url::Url>,
    /// Shared request redactor used by reconnect-generated errors.
    log_redactor: &'a Arc<HttpRedactor>,
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

/// Mutable reconnect counters and pending retry delay state.
struct ReconnectState {
    /// Number of reconnect sleeps already consumed.
    count: u32,
    /// Current local backoff delay before jitter/server override.
    backoff_delay: Duration,
    /// Optional one-shot server-provided retry delay from an SSE `retry:`
    /// record.
    pending_server_retry_delay: Option<Duration>,
}

impl ReconnectState {
    /// Creates reconnect state from retry options.
    ///
    /// # Parameters
    /// - `retry_options`: Retry options used to derive the initial delay.
    ///
    /// # Returns
    /// New reconnect state with zero consumed reconnects.
    fn new(retry_options: &RetryOptions) -> Self {
        Self {
            count: 0,
            backoff_delay: initial_reconnect_delay(retry_options),
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
        runtime: &ReconnectRuntime<'_>,
    ) -> ReconnectAction {
        let sleep_delay = self.sleep_delay(runtime);
        match reconnect_decision(
            self.count,
            runtime.max_reconnects,
            runtime.started_at,
            runtime.retry_options,
            sleep_delay,
        ) {
            ReconnectDecision::Allowed => {
                self.sleep_and_advance(sleep_delay, runtime).await
            }
            ReconnectDecision::MaxElapsedExceeded {
                elapsed,
                max_elapsed,
            } => ReconnectAction::Fail(Box::new(
                max_elapsed_exceeded_error_with_last_error(
                    error,
                    elapsed,
                    max_elapsed,
                    runtime.request_method,
                    runtime.request_url,
                    runtime.log_redactor,
                ),
            )),
            ReconnectDecision::MaxReconnectsReached => {
                ReconnectAction::Fail(Box::new(error))
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
        runtime: &ReconnectRuntime<'_>,
    ) -> ReconnectAction {
        let sleep_delay = self.sleep_delay(runtime);
        match reconnect_decision(
            self.count,
            runtime.max_reconnects,
            runtime.started_at,
            runtime.retry_options,
            sleep_delay,
        ) {
            ReconnectDecision::Allowed => {
                self.sleep_and_advance(sleep_delay, runtime).await
            }
            ReconnectDecision::MaxElapsedExceeded {
                elapsed,
                max_elapsed,
            } => ReconnectAction::Fail(Box::new(max_elapsed_exceeded_error(
                elapsed,
                max_elapsed,
                runtime.request_method,
                runtime.request_url,
                runtime.log_redactor,
            ))),
            ReconnectDecision::MaxReconnectsReached => ReconnectAction::Stop,
        }
    }

    /// Returns the effective sleep delay for the next reconnect.
    ///
    /// # Parameters
    /// - `runtime`: Shared reconnect policy context.
    ///
    /// # Returns
    /// Jittered or server-provided reconnect delay.
    fn sleep_delay(&self, runtime: &ReconnectRuntime<'_>) -> Duration {
        reconnect_sleep_delay(
            self.backoff_delay,
            self.pending_server_retry_delay,
            runtime.retry_options,
            runtime.options,
        )
    }

    /// Sleeps before reconnect and advances local reconnect state.
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
    /// Sleeps asynchronously and mutates reconnect counters/backoff state.
    async fn sleep_and_advance(
        &mut self,
        sleep_delay: Duration,
        runtime: &ReconnectRuntime<'_>,
    ) -> ReconnectAction {
        self.count += 1;
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
        self.backoff_delay =
            next_reconnect_delay(runtime.retry_options, self.backoff_delay);
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
            let retry_options = options.retry.clone();
            let max_reconnects = retry_options.max_attempts().saturating_sub(1);
            let request_url = request_template.resolved_url().ok();
            let request_method = request_template.method().clone();
            let cancellation_token = request_template.cancellation_token().cloned();
            let log_redactor = request_template.log_redactor().clone();
            let started_at = Instant::now();
            let runtime = ReconnectRuntime {
                retry_options: &retry_options,
                options: &options,
                max_reconnects,
                started_at,
                cancellation_token: cancellation_token.as_ref(),
                request_method: &request_method,
                request_url: request_url.as_ref(),
                log_redactor: &log_redactor,
            };
            let mut reconnect_state = ReconnectState::new(&retry_options);
            let mut last_event_id: Option<String> = None;
            loop {
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
                        yield Err(error);
                        return;
                    }
                }

                let response = match client.execute_once(request).await {
                    Ok(response) => response,
                    Err(error) => {
                        if should_reconnect_sse_error(&error) {
                            match reconnect_state.after_error(error, &runtime).await {
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
                    yield Err(error);
                    return;
                }

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
                                    &retry_options,
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
                        match reconnect_state.after_error(error, &runtime).await {
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
                    match reconnect_state.after_eof(&runtime).await {
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
    log_redactor: &Arc<HttpRedactor>,
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

/// Returns the next reconnect delay after one reconnect sleep.
///
/// # Parameters
/// - `retry_options`: Retry options for reconnect delay strategy.
/// - `current`: Current reconnect delay.
///
/// # Returns
/// Next reconnect base delay.
fn next_reconnect_delay(
    retry_options: &RetryOptions,
    current: Duration,
) -> Duration {
    retry_options
        .next_base_delay_from_current(current)
        .max(Duration::from_millis(1))
}

/// Returns one reconnect decision by checking reconnect count and elapsed-time
/// budget.
///
/// # Parameters
/// - `count`: Current reconnect count already consumed.
/// - `max_reconnects`: Maximum reconnect count.
/// - `started_at`: Runner start time.
/// - `retry_options`: Retry options containing optional elapsed-time budget.
/// - `sleep_delay`: Planned reconnect sleep duration for the next attempt.
///
/// # Returns
/// Reconnect decision for the next reconnect attempt.
fn reconnect_decision(
    count: u32,
    max_reconnects: u32,
    started_at: Instant,
    retry_options: &RetryOptions,
    sleep_delay: Duration,
) -> ReconnectDecision {
    if count >= max_reconnects {
        return ReconnectDecision::MaxReconnectsReached;
    }
    if let Some(max_elapsed) = retry_options.max_total_elapsed() {
        let elapsed = started_at.elapsed();
        if (elapsed >= max_elapsed)
            || will_exceed_elapsed(elapsed, sleep_delay, max_elapsed)
        {
            return ReconnectDecision::MaxElapsedExceeded {
                elapsed,
                max_elapsed,
            };
        }
    }
    ReconnectDecision::Allowed
}

/// Returns whether sleeping one more reconnect delay would exceed the elapsed
/// budget.
///
/// # Parameters
/// - `elapsed`: Already-consumed elapsed duration.
/// - `sleep_delay`: Planned reconnect sleep duration.
/// - `max_elapsed`: Total elapsed budget.
///
/// # Returns
/// `true` when `elapsed + sleep_delay` is greater than or equal to
/// `max_elapsed`, or when the addition overflows.
fn will_exceed_elapsed(
    elapsed: Duration,
    sleep_delay: Duration,
    max_elapsed: Duration,
) -> bool {
    elapsed
        .checked_add(sleep_delay)
        .is_none_or(|next_elapsed| next_elapsed >= max_elapsed)
}

/// Returns the initial reconnect delay from retry options.
///
/// # Parameters
/// - `retry_options`: Retry options for reconnect delay strategy.
///
/// # Returns
/// Initial reconnect base delay.
fn initial_reconnect_delay(retry_options: &RetryOptions) -> Duration {
    retry_options
        .base_delay_for_attempt(1)
        .max(Duration::from_millis(1))
}

/// Returns one reconnect sleep delay by applying configured jitter rules.
///
/// # Parameters
/// - `backoff_delay`: Base reconnect delay from local retry strategy.
/// - `pending_server_retry_delay`: Optional one-shot delay from SSE `retry:`.
/// - `retry_options`: Retry options containing jitter strategy.
/// - `options`: SSE reconnect options that control server-retry jitter
///   behavior.
///
/// # Returns
/// Effective sleep delay for the next reconnect wait.
fn reconnect_sleep_delay(
    backoff_delay: Duration,
    pending_server_retry_delay: Option<Duration>,
    retry_options: &RetryOptions,
    options: &SseReconnectOptions,
) -> Duration {
    let delay = if let Some(server_delay) = pending_server_retry_delay {
        if options.apply_jitter_to_server_retry {
            retry_options.jittered_delay(server_delay)
        } else {
            server_delay
        }
    } else {
        retry_options.jittered_delay(backoff_delay)
    };
    delay.max(Duration::from_millis(1))
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
    log_redactor: &Arc<HttpRedactor>,
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
    retry_options: &RetryOptions,
    options: &SseReconnectOptions,
) -> Duration {
    let raw = Duration::from_millis(retry_ms.max(1));
    let cap = server_retry_max_delay(retry_options, options);
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
    retry_options: &RetryOptions,
    options: &SseReconnectOptions,
) -> Duration {
    options
        .server_retry_max_delay
        .unwrap_or_else(|| default_server_retry_max_delay(retry_options))
        .max(Duration::from_millis(1))
}

/// Returns fallback server-retry cap derived from retry delay strategy.
///
/// # Parameters
/// - `retry_options`: Retry options whose delay strategy is inspected.
///
/// # Returns
/// Fallback cap for server-provided `retry:` delay.
fn default_server_retry_max_delay(retry_options: &RetryOptions) -> Duration {
    match retry_options.delay() {
        RetryDelay::None | RetryDelay::Fixed(_) => {
            DEFAULT_SSE_MAX_RECONNECT_DELAY
        }
        RetryDelay::Random { max, .. }
        | RetryDelay::Exponential { max, .. } => *max,
    }
    .max(Duration::from_millis(1))
}

/// Builds one reconnect max-elapsed error for reconnect-on-EOF path.
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
    log_redactor: &Arc<HttpRedactor>,
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
    log_redactor: &Arc<HttpRedactor>,
) -> HttpError {
    let mut error = max_elapsed_exceeded_error(
        elapsed,
        max_elapsed,
        request_method,
        request_url,
        log_redactor,
    );
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
    log_redactor: &Arc<HttpRedactor>,
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
