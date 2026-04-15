/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! HTTP retry executor used by `HttpClient` to run normal/stream requests.

use std::time::Duration;

use qubit_concurrent::Lock;
use qubit_retry::{RetryAttemptFailure, RetryError, RetryExecutor, RetryResult};

use crate::{
    HttpClient, HttpError, HttpRequest, HttpResponse, HttpResult, HttpRetryOptions,
    PendingHttpRetryAfterDelay,
};

/// HTTP retry executor for one resolved retry policy.
pub(super) struct HttpRetryExecutor {
    client: HttpClient,
    executor: RetryExecutor<HttpError>,
    pending_after_delay: Option<PendingHttpRetryAfterDelay>,
}

impl HttpRetryExecutor {
    /// Builds one HTTP retry executor from effective retry options.
    ///
    /// # Parameters
    /// - `client`: HTTP client used to run each attempt (stored by clone).
    /// - `retry_options`: Effective retry options for this request.
    /// - `honor_retry_after`: Whether to honor `Retry-After` on retryable
    ///   status responses (`429` and `5xx`).
    ///
    /// # Returns
    /// Configured HTTP retry executor.
    ///
    /// # Errors
    /// Returns [`HttpError`] when retry options or executor configuration is invalid.
    pub(super) fn new(
        client: &HttpClient,
        options: &HttpRetryOptions,
        honor_retry_after: bool,
    ) -> HttpResult<Self> {
        let (executor, pending_after_delay) = options.build_executor(honor_retry_after)?;
        Ok(Self {
            client: client.clone(),
            executor,
            pending_after_delay,
        })
    }

    /// Executes [`HttpClient::execute_once`] under the configured retry policy.
    ///
    /// # Parameters
    /// - `request`: Built request passed to each [`HttpClient::execute_once`]
    ///   attempt.
    ///
    /// # Returns
    /// Same as a successful single attempt, or a mapped [`HttpError`] when
    /// retries abort or limits are exceeded.
    pub(super) async fn execute(&self, request: HttpRequest) -> HttpResult<HttpResponse> {
        let client = self.client.clone();
        let pending_after_delay = self.pending_after_delay.clone();
        let result = self
            .executor
            .run_async(move || {
                let client = client.clone();
                let request = request.clone();
                let pending_after_delay = pending_after_delay.clone();
                async move {
                    if let Some(delay) = pending_after_delay
                        .as_ref()
                        .and_then(take_pending_after_delay)
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

/// Takes and clears the pending extra `Retry-After` delay.
///
/// # Parameters
/// - `pending`: Shared state carrying a pending delay.
///
/// # Returns
/// Pending delay if one exists.
fn take_pending_after_delay(pending: &PendingHttpRetryAfterDelay) -> Option<Duration> {
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

/// Maps a single retry [`RetryAttemptFailure`] into [`HttpResult`].
///
/// # Parameters
/// - `failure`: Single attempt outcome from the retry layer.
///
/// # Returns
/// Always `Err`: either the wrapped [`HttpError`] or a synthesized timeout
/// message.
fn map_retry_failure<T>(failure: RetryAttemptFailure<HttpError>) -> HttpResult<T> {
    Err(map_retry_failure_to_error(failure))
}

/// Converts a retry-layer attempt failure into [`HttpError`].
///
/// # Parameters
/// - `failure`: Attempt failure from the retry executor.
///
/// # Returns
/// Mapped [`HttpError`] with timeout context when applicable.
fn map_retry_failure_to_error(failure: RetryAttemptFailure<HttpError>) -> HttpError {
    match failure {
        RetryAttemptFailure::Error(error) => error,
        RetryAttemptFailure::AttemptTimeout { elapsed, timeout } => HttpError::other(format!(
            "HTTP retry attempt timeout after {elapsed:?} (timeout: {timeout:?})"
        )),
    }
}
