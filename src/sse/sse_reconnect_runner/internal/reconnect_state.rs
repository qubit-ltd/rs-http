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
pub(super) struct ReconnectState {
    /// Current local backoff delay before jitter/server override.
    pub(super) backoff: BackoffState,
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
    pub(super) fn new(retry_policy: &RetryPolicy) -> Self {
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
    pub(super) fn set_server_retry_delay(&mut self, delay: Duration) {
        self.pending_server_retry_delay = Some(delay);
    }

    /// Attempts to schedule a reconnect after a retryable error.
    ///
    /// # Parameters
    /// - `error`: Last retryable HTTP/SSE error.
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
    pub(super) async fn after_error(
        &mut self,
        error: HttpError,
        budget: &mut RetryBudget<'_>,
        runtime: &ReconnectRuntime<'_>,
    ) -> ReconnectAction {
        let sleep_delay = self.sleep_delay(runtime);
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
    pub(super) async fn after_eof(
        &mut self,
        budget: &mut RetryBudget<'_>,
        runtime: &ReconnectRuntime<'_>,
    ) -> ReconnectAction {
        let sleep_delay = self.sleep_delay(runtime);
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
    ///
    /// # Returns
    /// Jittered or server-provided reconnect delay.
    pub(super) fn sleep_delay(&mut self, runtime: &ReconnectRuntime<'_>) -> Duration {
        let request = match self.pending_server_retry_delay {
            Some(delay) if runtime.options.apply_jitter_to_server_retry => BackoffRequest::jittered_hint(delay),
            Some(delay) => BackoffRequest::hint(delay),
            None => BackoffRequest::policy(),
        };
        let delay = self
            .backoff
            .next(request)
            .effective_delay()
            .max(Duration::from_millis(1));
        if self.pending_server_retry_delay.is_some() {
            delay.min(server_retry_max_delay(runtime.retry_policy, runtime.options))
        } else {
            delay
        }
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
        self.pending_server_retry_delay = None;
        ReconnectAction::Continue
    }
}
