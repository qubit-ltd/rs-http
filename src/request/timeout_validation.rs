// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Request-level timeout validation helpers.

use std::time::Duration;
use std::time::Instant;

use crate::HttpError;
use crate::HttpResult;

/// Validates a request-level timeout duration.
///
/// # Parameters
/// - `name`: Public option or method name used in diagnostics.
/// - `timeout`: Duration to validate.
///
/// # Returns
/// `Ok(())` when `timeout` is greater than zero.
///
/// # Errors
/// Returns [`HttpError`] when `timeout` is zero.
pub(crate) fn validate_positive_timeout(name: &str, timeout: Duration) -> HttpResult<()> {
    if timeout.is_zero() {
        return Err(HttpError::other(format!("{name} must be greater than zero")));
    }
    Ok(())
}

/// Checks whether `timeout` can be added to the platform's current instant.
///
/// # Parameters
/// - `timeout`: Duration to add to the current instant.
///
/// # Returns
/// `true` when the duration is representable as an `Instant` deadline.
pub(crate) fn response_header_deadline_is_representable(timeout: Duration) -> bool {
    Instant::now().checked_add(timeout).is_some()
}

/// Validates that a response-header timeout is positive and representable.
///
/// # Parameters
/// - `timeout`: Response-header timeout to validate.
///
/// # Returns
/// `Ok(())` when `timeout` is valid.
///
/// # Errors
/// Returns [`HttpError`] when `timeout` is zero or cannot form an `Instant`
/// deadline on the current platform.
pub(crate) fn validate_response_header_timeout(timeout: Duration) -> HttpResult<()> {
    validate_positive_timeout("response_header_timeout", timeout)?;
    if !response_header_deadline_is_representable(timeout) {
        return Err(HttpError::other(
            "response_header_timeout exceeds the supported deadline range",
        ));
    }
    Ok(())
}
