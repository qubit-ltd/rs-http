// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Mutable backoff state and transitions for SSE reconnect scheduling.

use std::time::Duration;

use qubit_retry::BackoffRequest;
use qubit_retry::BackoffState;
use qubit_retry::RetryBudget;
use qubit_retry::RetryBudgetError;
use qubit_retry::RetryBudgetExhausted;
use qubit_retry::RetryPolicy;

use super::reconnect_action::ReconnectAction;
use super::reconnect_runtime::ReconnectRuntime;
use crate::HttpError;
use crate::sse::sse_reconnect_runner::max_elapsed_exceeded_error;
use crate::sse::sse_reconnect_runner::max_elapsed_exceeded_error_with_last_error;
use crate::sse::sse_reconnect_runner::operation_elapsed_exceeded_error;
use crate::sse::sse_reconnect_runner::operation_elapsed_exceeded_error_with_last_error;
use crate::sse::sse_reconnect_runner::reconnect_budget_error;
use crate::sse::sse_reconnect_runner::server_retry_max_delay;
use crate::sse::sse_reconnect_runner::sleep_reconnect_delay;

/// Mutable reconnect backoff and pending retry delay state.
pub(in crate::sse) struct ReconnectState {
    /// Current local backoff delay before jitter/server override.
    pub(in crate::sse) backoff: BackoffState,
    /// Persistent server-provided retry delay from the latest SSE `retry:`
    /// record.
    server_retry_delay: Option<Duration>,
}

impl ReconnectState {
    /// Creates reconnect state from retry policy backoff configuration.
    ///
    /// # Parameters
    /// - `retry_policy`: Retry policy used to derive the initial delay.
    ///
    /// # Returns
    /// New reconnect state with no pending server-provided delay.
    pub(in crate::sse) fn new(retry_policy: &RetryPolicy) -> Self {
        Self {
            backoff: retry_policy.backoff().start(),
            server_retry_delay: None,
        }
    }

    /// Stores the server-provided retry delay used for subsequent reconnects.
    ///
    /// # Parameters
    /// - `delay`: Delay derived from an SSE `retry:` record.
    ///
    /// # Returns
    /// Nothing.
    pub(in crate::sse) fn set_server_retry_delay(&mut self, delay: Duration) {
        self.server_retry_delay = Some(delay);
    }

    /// Attempts to schedule a reconnect after a retryable error.
    ///
    /// # Parameters
    /// - `error`: Last retryable HTTP/SSE error.
    /// - `honor_retry_after`: Whether an applicable HTTP `Retry-After` hint may
    ///   influence this reconnect delay. The hint is used only for status
    ///   errors with status `429` or a server-error status.
    /// - `budget`: Shared retry budget for this stream invocation.
    /// - `runtime`: Shared reconnect policy and diagnostic context.
    ///
    /// # Returns
    /// Reconnect action for the caller.
    ///
    /// # Side effects
    /// Sleeps asynchronously when another reconnect is allowed.
    ///
    /// # Panics
    /// Panics if the retry budget reports total elapsed exhaustion while the
    /// retry policy has no configured total time limit.
    pub(in crate::sse) async fn after_error(
        &mut self,
        error: HttpError,
        honor_retry_after: bool,
        budget: &mut RetryBudget<'_>,
        runtime: &ReconnectRuntime<'_>,
    ) -> ReconnectAction {
        let http_retry_after = http_retry_after(&error, honor_retry_after);
        let sleep_delay = self.sleep_delay(runtime, http_retry_after);
        let decision = budget.check_retry_after(sleep_delay);
        let snapshot = match budget.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return ReconnectAction::Fail(Box::new(reconnect_budget_error(error, runtime)));
            }
        };
        match decision {
            Ok(()) => self.sleep_and_advance(sleep_delay, runtime).await,
            Err(RetryBudgetError::Exhausted(RetryBudgetExhausted::Attempts)) => ReconnectAction::Fail(Box::new(error)),
            Err(RetryBudgetError::Exhausted(RetryBudgetExhausted::OperationElapsed)) => {
                ReconnectAction::Fail(Box::new(operation_elapsed_exceeded_error_with_last_error(
                    error,
                    snapshot.operation_elapsed(),
                    runtime.retry_policy.admission_limits().operation_time_budget(),
                    runtime.request_method,
                    runtime.request_url,
                    runtime.log_redactor,
                )))
            }
            Err(RetryBudgetError::Exhausted(RetryBudgetExhausted::TotalElapsed)) => {
                ReconnectAction::Fail(Box::new(max_elapsed_exceeded_error_with_last_error(
                    error,
                    snapshot.total_elapsed(),
                    runtime
                        .retry_policy
                        .admission_limits()
                        .total_time_budget()
                        .expect("total budget exhaustion requires a configured total limit"),
                    runtime.request_method,
                    runtime.request_url,
                    runtime.log_redactor,
                )))
            }
            Err(error) => ReconnectAction::Fail(Box::new(reconnect_budget_error(error, runtime))),
        }
    }

    /// Attempts to schedule a reconnect after a clean stream EOF.
    ///
    /// # Parameters
    /// - `budget`: Shared retry budget for this stream invocation.
    /// - `runtime`: Shared reconnect policy and diagnostic context.
    ///
    /// # Returns
    /// Reconnect action for the caller.
    ///
    /// # Side effects
    /// Sleeps asynchronously when another reconnect is allowed.
    ///
    /// # Panics
    /// Panics if the retry budget reports total elapsed exhaustion while the
    /// retry policy has no configured total time limit.
    pub(in crate::sse) async fn after_eof(
        &mut self,
        budget: &mut RetryBudget<'_>,
        runtime: &ReconnectRuntime<'_>,
    ) -> ReconnectAction {
        let sleep_delay = self.sleep_delay(runtime, None);
        let decision = budget.check_retry_after(sleep_delay);
        let snapshot = match budget.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return ReconnectAction::Fail(Box::new(reconnect_budget_error(error, runtime)));
            }
        };
        match decision {
            Ok(()) => self.sleep_and_advance(sleep_delay, runtime).await,
            Err(RetryBudgetError::Exhausted(RetryBudgetExhausted::Attempts)) => ReconnectAction::Stop,
            Err(RetryBudgetError::Exhausted(RetryBudgetExhausted::OperationElapsed)) => {
                ReconnectAction::Fail(Box::new(operation_elapsed_exceeded_error(
                    snapshot.operation_elapsed(),
                    runtime.retry_policy.admission_limits().operation_time_budget(),
                    runtime.request_method,
                    runtime.request_url,
                    runtime.log_redactor,
                )))
            }
            Err(RetryBudgetError::Exhausted(RetryBudgetExhausted::TotalElapsed)) => {
                ReconnectAction::Fail(Box::new(max_elapsed_exceeded_error(
                    snapshot.total_elapsed(),
                    runtime
                        .retry_policy
                        .admission_limits()
                        .total_time_budget()
                        .expect("total budget exhaustion requires a configured total limit"),
                    runtime.request_method,
                    runtime.request_url,
                    runtime.log_redactor,
                )))
            }
            Err(error) => ReconnectAction::Fail(Box::new(reconnect_budget_error(error, runtime))),
        }
    }

    /// Returns the effective sleep delay for the next reconnect.
    ///
    /// # Parameters
    /// - `runtime`: Shared reconnect policy context.
    /// - `http_retry_after`: Optional applicable HTTP `Retry-After` delay. When
    ///   present, it takes precedence over a stored SSE `retry:` delay and the
    ///   local backoff policy for this wait. The configured backoff policy
    ///   still processes the hint, and the resulting delay remains subject to
    ///   retry-budget admission. The SSE-specific maximum delay applies only to
    ///   SSE hints.
    ///
    /// # Returns
    /// Effective reconnect delay after backoff-policy processing. This method
    /// does not clear a stored SSE retry delay when an HTTP hint is supplied.
    pub(in crate::sse) fn sleep_delay(
        &mut self,
        runtime: &ReconnectRuntime<'_>,
        http_retry_after: Option<Duration>,
    ) -> Duration {
        let (request, from_sse) = if let Some(delay) = http_retry_after {
            (BackoffRequest::hint(delay), false)
        } else if let Some(delay) = self.server_retry_delay {
            let request = if runtime.options.apply_jitter_to_server_retry {
                BackoffRequest::jittered_hint(delay)
            } else {
                BackoffRequest::hint(delay)
            };
            (request, true)
        } else {
            (BackoffRequest::policy(), false)
        };
        let delay = self
            .backoff
            .next(request)
            .effective_delay()
            .max(Duration::from_millis(1));
        if from_sse {
            delay.min(server_retry_max_delay(runtime.retry_policy, runtime.options))
        } else {
            delay
        }
    }

    /// Sleeps before reconnect and advances the local backoff state.
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
    /// Sleeps asynchronously while preserving the latest server retry delay.
    async fn sleep_and_advance(&mut self, sleep_delay: Duration, runtime: &ReconnectRuntime<'_>) -> ReconnectAction {
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
        ReconnectAction::Continue
    }
}

/// Returns an applicable HTTP `Retry-After` delay for an SSE reconnect.
///
/// The header is considered only when the caller honors retry hints and the
/// error is a status error with status `429 Too Many Requests` or a 5xx server
/// error. Other errors and statuses do not provide an HTTP delay.
///
/// # Parameters
/// - `error`: HTTP error whose status and parsed retry delay are inspected.
/// - `honor_retry_after`: Whether HTTP retry hints are enabled for this request
///   attempt.
///
/// # Returns
/// The parsed HTTP retry delay when all conditions apply; otherwise `None`.
/// The caller passes a returned delay through the configured backoff policy
/// and retry-budget checks before scheduling a reconnect. This function does
/// not modify a persistent SSE `retry:` delay.
fn http_retry_after(error: &HttpError, honor_retry_after: bool) -> Option<Duration> {
    if honor_retry_after
        && error.kind() == crate::HttpErrorKind::Status
        && error
            .status()
            .is_some_and(|status| status == http::StatusCode::TOO_MANY_REQUESTS || status.is_server_error())
    {
        error.retry_after()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http::Method;
    use qubit_clock::StdMonotonicClock;
    use qubit_redact::Redactor;
    use qubit_retry::BackoffPolicy;
    use qubit_retry::RetryBudget;

    use super::ReconnectState;
    use crate::HttpError;
    use crate::sse::SseReconnectOptions;
    use crate::sse::sse_reconnect_runner::internal::reconnect_runtime::ReconnectRuntime;

    fn delay_for_hint(retry: qubit_retry::RetryPolicy, options: SseReconnectOptions, hint: Duration) -> Duration {
        let mut state = ReconnectState::new(&retry);
        state.set_server_retry_delay(hint);
        let method = Method::GET;
        let redactor = Redactor::default();
        let runtime = ReconnectRuntime {
            retry_policy: &retry,
            options: &options,
            cancellation_token: None,
            request_method: &method,
            request_url: None,
            log_redactor: &redactor,
        };
        state.sleep_delay(&runtime, None)
    }

    fn retry(backoff: BackoffPolicy) -> qubit_retry::RetryPolicy {
        qubit_retry::RetryPolicy::builder()
            .max_attempts(2)
            .backoff(backoff)
            .build()
            .expect("test retry policy should be valid")
    }

    #[test]
    fn test_sleep_delay_uses_explicit_server_retry_cap() {
        let options = SseReconnectOptions {
            retry: retry(BackoffPolicy::fixed(Duration::from_millis(1))),
            server_retry_max_delay: Some(Duration::from_millis(80)),
            apply_jitter_to_server_retry: false,
            ..SseReconnectOptions::default()
        };
        let delay = delay_for_hint(options.retry.clone(), options, Duration::from_millis(800));
        assert_eq!(delay, Duration::from_millis(80));
    }

    #[test]
    fn test_sleep_delay_derives_server_retry_cap_from_backoff() {
        let policy = retry(
            BackoffPolicy::uniform(Duration::from_millis(20), Duration::from_millis(80))
                .expect("test backoff should be valid"),
        );
        let options = SseReconnectOptions {
            retry: policy.clone(),
            server_retry_max_delay: None,
            apply_jitter_to_server_retry: false,
            ..SseReconnectOptions::default()
        };
        assert_eq!(
            delay_for_hint(policy, options, Duration::from_millis(800)),
            Duration::from_millis(80)
        );
    }

    #[test]
    fn test_sleep_delay_enforces_one_millisecond_minimum() {
        let policy = retry(BackoffPolicy::fixed(Duration::ZERO));
        let options = SseReconnectOptions {
            retry: policy.clone(),
            server_retry_max_delay: Some(Duration::ZERO),
            apply_jitter_to_server_retry: false,
            ..SseReconnectOptions::default()
        };
        assert_eq!(
            delay_for_hint(policy, options, Duration::ZERO),
            Duration::from_millis(1)
        );
    }

    #[test]
    fn test_sleep_delay_applies_jitter_without_exceeding_server_cap() {
        let policy = retry(
            BackoffPolicy::fixed(Duration::from_millis(1))
                .with_bounded_jitter(1.0)
                .expect("test jitter should be valid"),
        );
        let options = SseReconnectOptions {
            retry: policy.clone(),
            server_retry_max_delay: Some(Duration::from_millis(80)),
            apply_jitter_to_server_retry: true,
            ..SseReconnectOptions::default()
        };
        let delay = delay_for_hint(policy, options, Duration::from_millis(800));
        assert!(delay >= Duration::from_millis(1), "delay={delay:?}");
        assert!(delay <= Duration::from_millis(80), "delay={delay:?}");
    }

    #[test]
    fn test_sleep_delay_preserves_exact_server_hint_without_jitter() {
        let policy = retry(BackoffPolicy::fixed(Duration::from_millis(1)));
        let options = SseReconnectOptions {
            retry: policy.clone(),
            server_retry_max_delay: Some(Duration::from_millis(200)),
            apply_jitter_to_server_retry: false,
            ..SseReconnectOptions::default()
        };
        assert_eq!(
            delay_for_hint(policy, options, Duration::from_millis(120)),
            Duration::from_millis(120)
        );
    }

    #[test]
    fn test_http_retry_after_hint_is_not_limited_by_sse_server_cap() {
        let policy = retry(BackoffPolicy::fixed(Duration::from_secs(1)).prefer_retry_after());
        let options = SseReconnectOptions {
            server_retry_max_delay: Some(Duration::from_millis(50)),
            ..SseReconnectOptions::default()
        };
        let method = Method::GET;
        let redactor = Redactor::default();
        let runtime = ReconnectRuntime {
            retry_policy: &policy,
            options: &options,
            cancellation_token: None,
            request_method: &method,
            request_url: None,
            log_redactor: &redactor,
        };
        let mut state = ReconnectState::new(&policy);
        assert_eq!(
            state.sleep_delay(&runtime, Some(Duration::from_millis(120))),
            Duration::from_millis(120)
        );
        assert_eq!(state.sleep_delay(&runtime, None), Duration::from_secs(1));
        assert_eq!(
            state.sleep_delay(&runtime, Some(Duration::ZERO)),
            Duration::from_millis(1)
        );

        let limited_policy = retry(
            BackoffPolicy::fixed(Duration::from_secs(1))
                .prefer_retry_after()
                .limit_delay(Duration::from_millis(10)),
        );
        let limited_options = SseReconnectOptions {
            retry: limited_policy.clone(),
            ..SseReconnectOptions::default()
        };
        let limited_runtime = ReconnectRuntime {
            retry_policy: &limited_policy,
            options: &limited_options,
            cancellation_token: None,
            request_method: &method,
            request_url: None,
            log_redactor: &redactor,
        };
        let mut limited_state = ReconnectState::new(&limited_policy);
        assert_eq!(
            limited_state.sleep_delay(&limited_runtime, Some(Duration::from_millis(120))),
            Duration::from_millis(10)
        );
    }

    #[test]
    fn test_http_retry_after_requires_status_and_final_preference() {
        let retry_after = Duration::from_secs(120);
        for status in [
            http::StatusCode::TOO_MANY_REQUESTS,
            http::StatusCode::SERVICE_UNAVAILABLE,
        ] {
            let error = HttpError::from_status(status, "retry").with_retry_after(retry_after);
            assert_eq!(super::http_retry_after(&error, true), Some(retry_after));
            assert_eq!(super::http_retry_after(&error, false), None);
        }
        let error = HttpError::transport("retry").with_retry_after(retry_after);
        assert_eq!(super::http_retry_after(&error, true), None);
        let error = HttpError::from_status(http::StatusCode::BAD_REQUEST, "retry").with_retry_after(retry_after);
        assert_eq!(super::http_retry_after(&error, true), None);
    }

    #[tokio::test(start_paused = true)]
    async fn test_wait_preserves_server_retry_and_advances_backoff() {
        let policy = qubit_retry::RetryPolicy::builder()
            .max_attempts(3)
            .backoff(BackoffPolicy::exponential(Duration::from_millis(40), 2.0, Duration::from_millis(200)).unwrap())
            .build()
            .unwrap();
        let options = SseReconnectOptions {
            apply_jitter_to_server_retry: false,
            ..SseReconnectOptions::default()
        };
        let method = Method::GET;
        let redactor = Redactor::default();
        let runtime = ReconnectRuntime {
            retry_policy: &policy,
            options: &options,
            cancellation_token: None,
            request_method: &method,
            request_url: None,
            log_redactor: &redactor,
        };
        let clock = StdMonotonicClock::new();
        let mut budget = RetryBudget::new(&clock, *policy.admission_limits()).unwrap();
        let mut state = ReconnectState::new(&policy);
        state.set_server_retry_delay(Duration::from_millis(120));
        for expected_ms in [120_u64, 120_u64] {
            let attempt = budget.begin_attempt().unwrap();
            let _snapshot = budget.finish_attempt(attempt).unwrap();
            {
                let wait = state.after_eof(&mut budget, &runtime);
                tokio::pin!(wait);
                assert!(futures_util::poll!(wait.as_mut()).is_pending());
                tokio::time::advance(Duration::from_millis(expected_ms - 1)).await;
                assert!(futures_util::poll!(wait.as_mut()).is_pending());
                tokio::time::advance(Duration::from_millis(1)).await;
                assert!(matches!(
                    futures_util::poll!(wait.as_mut()),
                    std::task::Poll::Ready(super::ReconnectAction::Continue)
                ));
            }
            assert_eq!(state.server_retry_delay, Some(Duration::from_millis(120)));
        }
    }

    #[test]
    fn test_http_retry_after_does_not_clear_persistent_sse_delay() {
        let policy = retry(BackoffPolicy::fixed(Duration::from_millis(5)).prefer_retry_after());
        let options = SseReconnectOptions {
            retry: policy.clone(),
            server_retry_max_delay: Some(Duration::from_millis(500)),
            apply_jitter_to_server_retry: false,
            ..SseReconnectOptions::default()
        };
        let method = Method::GET;
        let redactor = Redactor::default();
        let runtime = ReconnectRuntime {
            retry_policy: &policy,
            options: &options,
            cancellation_token: None,
            request_method: &method,
            request_url: None,
            log_redactor: &redactor,
        };
        let mut state = ReconnectState::new(&policy);
        state.set_server_retry_delay(Duration::from_millis(120));
        assert_eq!(state.sleep_delay(&runtime, None), Duration::from_millis(120));
        assert_eq!(state.sleep_delay(&runtime, None), Duration::from_millis(120));
        assert_eq!(
            state.sleep_delay(&runtime, Some(Duration::from_millis(250))),
            Duration::from_millis(250),
        );
        assert_eq!(state.sleep_delay(&runtime, None), Duration::from_millis(120));
        state.set_server_retry_delay(Duration::from_millis(40));
        assert_eq!(state.sleep_delay(&runtime, None), Duration::from_millis(40));
    }

    #[tokio::test(start_paused = true)]
    async fn test_cancelled_http_retry_after_wait_fails_as_cancelled() {
        let policy = retry(BackoffPolicy::fixed(Duration::from_secs(1)).prefer_retry_after());
        let options = SseReconnectOptions {
            retry: policy.clone(),
            ..SseReconnectOptions::default()
        };
        let method = Method::GET;
        let redactor = Redactor::default();
        let cancellation = crate::HttpCancellationToken::new();
        cancellation.cancel();
        let runtime = ReconnectRuntime {
            retry_policy: &policy,
            options: &options,
            cancellation_token: Some(&cancellation),
            request_method: &method,
            request_url: None,
            log_redactor: &redactor,
        };
        let clock = StdMonotonicClock::new();
        let mut budget = RetryBudget::new(&clock, *policy.admission_limits()).unwrap();
        let attempt = budget.begin_attempt().unwrap();
        let _snapshot = budget.finish_attempt(attempt).unwrap();
        let mut state = ReconnectState::new(&policy);
        let action = state
            .after_error(
                HttpError::from_status(http::StatusCode::SERVICE_UNAVAILABLE, "retry")
                    .with_retry_after(Duration::from_secs(10)),
                true,
                &mut budget,
                &runtime,
            )
            .await;
        let super::ReconnectAction::Fail(error) = action else {
            panic!("cancellation should interrupt the HTTP Retry-After wait");
        };
        assert_eq!(error.kind(), crate::HttpErrorKind::Cancelled);
    }
}
