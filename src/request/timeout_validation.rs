// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Request-level timeout validation helpers.

use std::time::Duration;

use crate::{HttpError, HttpResult};

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
        return Err(HttpError::other(format!(
            "{name} must be greater than zero"
        )));
    }
    Ok(())
}
