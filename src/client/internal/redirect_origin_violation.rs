// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Error used when an HTTP redirect violates the configured origin policy.

use std::error::Error;
use std::fmt;

/// Error returned by the redirect policy when a redirect crosses origins.
#[derive(Debug)]
pub(crate) struct RedirectOriginViolation;

impl fmt::Display for RedirectOriginViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("redirect target violates the same-origin policy")
    }
}

impl Error for RedirectOriginViolation {}
