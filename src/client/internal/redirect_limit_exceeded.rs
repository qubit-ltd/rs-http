// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Error used when a redirect exceeds the configured limit.

use std::error::Error;
use std::fmt;

/// Error returned by the redirect policy after the redirect limit is reached.
#[derive(Debug)]
pub(crate) struct RedirectLimitExceeded;

impl fmt::Display for RedirectLimitExceeded {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("redirect limit exceeded")
    }
}

impl Error for RedirectLimitExceeded {}
