/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! [`SseReconnectRunner`] implementation used by [`HttpClient`](crate::HttpClient).

use std::error::Error as StdError;
use std::io::ErrorKind;
use std::time::Duration;

use async_stream::stream;
use futures_util::StreamExt;
use http::header::{HeaderName, HeaderValue};

use super::{
    SseEventStream, SseReconnectOptions, DEFAULT_SSE_MAX_RECONNECT_DELAY,
    DEFAULT_SSE_RECONNECT_BACKOFF_MULTIPLIER,
};
use crate::{
    HttpClient, HttpError, HttpErrorKind, HttpRequest, HttpResult, RetryDelay, RetryHint,
    RetryJitter, RetryOptions,
};

/// Header name used for SSE resume token propagation.
const LAST_EVENT_ID_HEADER: &str = "last-event-id";

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

    /// Starts the reconnect loop and returns a merged SSE event stream.
    ///
    /// # Returns
    /// SSE event stream yielding events from one or more reconnect sessions.
    pub(crate) fn run(self) -> SseEventStream {
        let client = self.client;
        let request_template = self.request_template;
        let options = self.options;
        let output = stream! {
            let retry_options = normalize_retry_options(options.retry);
            let max_reconnects = retry_options.max_attempts.get().saturating_sub(1);
            let mut count: u32 = 0;
            let mut delay = initial_reconnect_delay(&retry_options);
            let mut last_event_id: Option<String> = None;
            loop {
                let mut request = request_template.clone();
                if let Some(last_event_id) = last_event_id.as_deref() {
                    if let Err(error) = apply_last_event_id_header(&mut request, last_event_id) {
                        yield Err(error);
                        return;
                    }
                }

                let response = match client.execute(request).await {
                    Ok(response) => response,
                    Err(error) => {
                        if (count < max_reconnects) && should_reconnect_sse_error(&error) {
                            count += 1;
                            tokio::time::sleep(retry_options.jittered_delay(delay)).await;
                            delay = next_reconnect_delay(&retry_options, delay);
                            continue;
                        }
                        yield Err(error);
                        return;
                    }
                };

                let mut events = response.sse_events();
                let mut stream_error: Option<HttpError> = None;
                while let Some(item) = events.next().await {
                    match item {
                        Ok(event) => {
                            if let Some(id) = event.id.clone() {
                                last_event_id = Some(id);
                            }
                            if options.honor_server_retry {
                                if let Some(retry_ms) = event.retry {
                                    delay = Duration::from_millis(retry_ms.max(1));
                                }
                            }
                            yield Ok(event);
                        }
                        Err(error) => {
                            stream_error = Some(error);
                            break;
                        }
                    }
                }

                if let Some(error) = stream_error {
                    if (count < max_reconnects) && should_reconnect_sse_error(&error) {
                        count += 1;
                        tokio::time::sleep(retry_options.jittered_delay(delay)).await;
                        delay = next_reconnect_delay(&retry_options, delay);
                        continue;
                    }
                    yield Err(error);
                    return;
                }

                if options.reconnect_on_eof && (count < max_reconnects) {
                    count += 1;
                    tokio::time::sleep(retry_options.jittered_delay(delay)).await;
                    delay = next_reconnect_delay(&retry_options, delay);
                    continue;
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
fn apply_last_event_id_header(request: &mut HttpRequest, last_event_id: &str) -> HttpResult<()> {
    let header_value = HeaderValue::from_str(last_event_id).map_err(|error| {
        HttpError::other(format!(
            "Invalid Last-Event-ID header value '{last_event_id}': {error}"
        ))
    })?;
    request.set_typed_header(HeaderName::from_static(LAST_EVENT_ID_HEADER), header_value);
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
    matches!(error.retry_hint(), RetryHint::Retryable) || is_unexpected_eof_error(error)
}

/// Returns the next reconnect delay after one reconnect sleep.
///
/// # Parameters
/// - `retry_options`: Retry options for reconnect delay strategy.
/// - `current`: Current reconnect delay.
///
/// # Returns
/// Next reconnect base delay.
fn next_reconnect_delay(retry_options: &RetryOptions, current: Duration) -> Duration {
    retry_options
        .next_base_delay_from_current(current)
        .max(Duration::from_millis(1))
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

/// Normalizes retry options from reconnect settings.
///
/// # Parameters
/// - `value`: Raw retry options supplied in [`SseReconnectOptions`].
///
/// # Returns
/// Valid retry options, falling back to SSE reconnect defaults when invalid.
fn normalize_retry_options(mut value: RetryOptions) -> RetryOptions {
    if value.delay.validate().is_err() {
        value.delay = RetryDelay::exponential(
            Duration::from_secs(1),
            DEFAULT_SSE_MAX_RECONNECT_DELAY,
            DEFAULT_SSE_RECONNECT_BACKOFF_MULTIPLIER,
        );
    }
    if value.jitter.validate().is_err() {
        value.jitter = RetryJitter::None;
    }
    value
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
    let contains_unexpected_eof = |text: &str| text.to_ascii_lowercase().contains("unexpected eof");
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
