/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Retry controller used by `HttpClient` to run normal/stream requests.

use std::time::Duration;

use qubit_concurrent::{ArcMutex, Lock};
use qubit_retry::{
    AttemptFailure, Jitter, RetryDecision, RetryError, RetryExecutor, RetryOptions, RetryResult,
};

use crate::{
    HttpClient, HttpError, HttpErrorKind, HttpRequest, HttpResponse, HttpResult, HttpRetryOptions,
};

/// Shared state used to carry extra `Retry-After` delay into the next async
/// retry attempt.
type PendingRetryAfterDelay = ArcMutex<Option<Duration>>;

/// Retry coordinator for one resolved retry policy.
pub(super) struct RetryController {
    executor: RetryExecutor<HttpError>,
    pending_retry_after_delay: Option<PendingRetryAfterDelay>,
}

impl RetryController {
    /// Builds one retry controller from effective retry options.
    ///
    /// # Parameters
    /// - `retry_options`: Effective retry options for this request.
    /// - `honor_retry_after`: Whether to honor `Retry-After` on retryable
    ///   status responses (`429` and `5xx`).
    ///
    /// # Returns
    /// Configured retry controller.
    ///
    /// # Errors
    /// Returns [`HttpError`] when retry options or executor configuration is invalid.
    pub(super) fn new(
        retry_options: &HttpRetryOptions,
        honor_retry_after: bool,
    ) -> HttpResult<Self> {
        let options = RetryOptions::new(
            retry_options.max_attempts,
            retry_options.max_duration,
            retry_options.delay_strategy.clone(),
            Jitter::factor(retry_options.jitter_factor),
        )
        .map_err(|error| HttpError::other(format!("Invalid HTTP retry options: {error}")))?;

        let retry_options_clone = retry_options.clone();
        let mut builder = RetryExecutor::<HttpError>::builder()
            .options(options)
            .classify_error(move |error: &HttpError, _| {
                let retryable = if error.kind == HttpErrorKind::Status {
                    error
                        .status
                        .is_some_and(|status| retry_options_clone.is_retryable_status(status))
                } else {
                    retry_options_clone.is_retryable_error_kind(error.kind)
                };
                if retryable {
                    RetryDecision::Retry
                } else {
                    RetryDecision::Abort
                }
            });

        if honor_retry_after {
            let pending_retry_after_delay: PendingRetryAfterDelay = ArcMutex::new(None);
            let pending_for_listener = pending_retry_after_delay.clone();
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
                .map(|executor| Self {
                    executor,
                    pending_retry_after_delay: Some(pending_retry_after_delay),
                })
                .map_err(|error| {
                    HttpError::other(format!("Invalid HTTP retry executor: {error}"))
                });
        }

        builder
            .build()
            .map(|executor| Self {
                executor,
                pending_retry_after_delay: None,
            })
            .map_err(|error| HttpError::other(format!("Invalid HTTP retry executor: {error}")))
    }

    /// Runs [`HttpClient::execute_once`] under the configured retry policy.
    ///
    /// # Parameters
    /// - `client`: HTTP client used to run attempts.
    /// - `request`: Built request passed to each [`HttpClient::execute_once`]
    ///   attempt.
    ///
    /// # Returns
    /// Same as a successful single attempt, or a mapped [`HttpError`] when
    /// retries abort or limits are exceeded.
    pub(super) async fn run_response(
        &self,
        client: &HttpClient,
        request: HttpRequest,
    ) -> HttpResult<HttpResponse> {
        let client = client.clone();
        let pending_retry_after_delay = self.pending_retry_after_delay.clone();
        let result = self
            .executor
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

}

/// Stores the extra `Retry-After` delay that should be applied before the next
/// retry attempt.
///
/// # Parameters
/// - `pending`: Shared state carrying a pending delay.
/// - `delay`: Extra delay to store.
fn set_pending_retry_after_delay(pending: &PendingRetryAfterDelay, delay: Duration) {
    pending.write(|slot| *slot = Some(delay));
}

/// Takes and clears the pending extra `Retry-After` delay.
///
/// # Parameters
/// - `pending`: Shared state carrying a pending delay.
///
/// # Returns
/// Pending delay if one exists.
fn take_pending_retry_after_delay(pending: &PendingRetryAfterDelay) -> Option<Duration> {
    pending.write(Option::take)
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
