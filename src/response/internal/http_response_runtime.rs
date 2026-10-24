// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Runtime state and repeatable diagnostics for one HTTP response.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use url::Url;

use super::body_read_failure::BodyReadFailure;
use super::body_read_failure::BodyReadFailureState;
use crate::HttpCancellationToken;
use crate::HttpError;

/// Runtime state bound to one response instance.
#[derive(Debug, Clone)]
pub(super) struct HttpResponseRuntime {
    /// Per-response read timeout inherited from request/client.
    pub(super) read_timeout: Duration,
    /// Optional cancellation token inherited from request.
    pub(super) cancellation_token: Option<HttpCancellationToken>,
    /// Request URL used in read/cancellation error context.
    pub(super) request_url: Url,
    /// First response body read failure, if the backend stream failed after
    /// being taken from this response.
    pub(super) body_read_failure: BodyReadFailureState,
}

impl HttpResponseRuntime {
    /// Creates runtime state for one response body.
    ///
    /// # Parameters
    /// - `read_timeout`: Maximum wait for each body read.
    /// - `cancellation_token`: Optional token inherited from the request.
    /// - `request_url`: URL used in read and cancellation diagnostics.
    ///
    /// # Returns
    /// Runtime state with no recorded body-read failure.
    pub(super) fn new(
        read_timeout: Duration,
        cancellation_token: Option<HttpCancellationToken>,
        request_url: Url,
    ) -> Self {
        Self {
            read_timeout,
            cancellation_token,
            request_url,
            body_read_failure: Arc::new(Mutex::new(None)),
        }
    }
}

/// Stores the first body-read failure in a shared state holder.
///
/// # Parameters
/// - `state`: Shared failure state captured by response streams.
/// - `error`: Error produced while reading the response body.
pub(super) fn remember_body_read_failure_state(state: &BodyReadFailureState, error: &HttpError) {
    if let Ok(mut guard) = state.lock() {
        guard.get_or_insert_with(|| BodyReadFailure::from_error(error));
    }
}

/// Returns a previously stored body-read failure, if any.
///
/// # Parameters
/// - `state`: Shared failure state retained by the response.
///
/// # Returns
/// Reconstructed error when a prior body read failed, otherwise `None`.
pub(super) fn previous_body_read_error(state: &BodyReadFailureState) -> Option<HttpError> {
    let guard = state.lock().ok()?;
    guard.as_ref().map(BodyReadFailure::to_error)
}
