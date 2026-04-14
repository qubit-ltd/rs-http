/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! SSE reconnect runner used by `HttpClient`.

use std::time::Duration;

use async_stream::stream;
use futures_util::StreamExt;
use http::header::{HeaderName, HeaderValue};

use crate::sse::{SseEventStream, SseReconnectOptions};
use crate::{HttpClient, HttpError, HttpErrorKind, HttpRequest, HttpResult, RetryHint};

/// Header name used for SSE resume token propagation.
const LAST_EVENT_ID_HEADER: &str = "last-event-id";

/// Stateful SSE reconnect runner for one stream invocation.
pub(super) struct SseReconnectRunner {
    client: HttpClient,
    request_template: HttpRequest,
    reconnect_options: SseReconnectOptions,
}

impl SseReconnectRunner {
    /// Creates a reconnect runner bound to one request template.
    ///
    /// # Parameters
    /// - `client`: HTTP client used to open each stream attempt.
    /// - `request_template`: Request reused across reconnect attempts.
    /// - `reconnect_options`: Reconnect limits and delay policy.
    ///
    /// # Returns
    /// New SSE reconnect runner.
    pub(super) fn new(
        client: HttpClient,
        request_template: HttpRequest,
        reconnect_options: SseReconnectOptions,
    ) -> Self {
        Self {
            client,
            request_template,
            reconnect_options,
        }
    }

    /// Starts the reconnect loop and returns a merged SSE event stream.
    ///
    /// # Returns
    /// SSE event stream yielding events from one or more reconnect sessions.
    pub(super) fn run(self) -> SseEventStream {
        let client = self.client;
        let request_template = self.request_template;
        let reconnect_options = self.reconnect_options;
        let output = stream! {
            let mut reconnect_count: u32 = 0;
            let mut reconnect_delay = reconnect_options.reconnect_delay;
            let mut last_event_id: Option<String> = None;
            loop {
                let mut attempt_request = request_template.clone();
                if let Some(last_event_id) = last_event_id.as_deref() {
                    if let Err(error) = apply_last_event_id_header(&mut attempt_request, last_event_id) {
                        yield Err(error);
                        return;
                    }
                }

                let response = match client.execute_stream(attempt_request).await {
                    Ok(response) => response,
                    Err(error) => {
                        if should_reconnect_sse_error(&error)
                            && reconnect_count < reconnect_options.max_reconnects {
                            reconnect_count += 1;
                            tokio::time::sleep(reconnect_delay).await;
                            continue;
                        }
                        yield Err(error);
                        return;
                    }
                };

                let mut events = response.decode_events();
                let mut stream_error: Option<HttpError> = None;
                while let Some(item) = events.next().await {
                    match item {
                        Ok(event) => {
                            if let Some(id) = event.id.clone() {
                                last_event_id = Some(id);
                            }
                            if reconnect_options.honor_server_retry {
                                if let Some(retry_ms) = event.retry {
                                    reconnect_delay = Duration::from_millis(retry_ms.max(1));
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
                    if should_reconnect_sse_error(&error)
                        && reconnect_count < reconnect_options.max_reconnects {
                        reconnect_count += 1;
                        tokio::time::sleep(reconnect_delay).await;
                        continue;
                    }
                    yield Err(error);
                    return;
                }

                if reconnect_options.reconnect_on_eof
                    && reconnect_count < reconnect_options.max_reconnects {
                    reconnect_count += 1;
                    tokio::time::sleep(reconnect_delay).await;
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
    request
        .headers
        .insert(HeaderName::from_static(LAST_EVENT_ID_HEADER), header_value);
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
        contains_unexpected_eof(&source.to_string())
            || contains_unexpected_eof(&format!("{source:?}"))
    })
}
