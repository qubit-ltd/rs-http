// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Error used when an IPv4-only client is redirected to an IPv6 literal.

use std::error::Error;
use std::fmt;

/// Error returned when an IPv4-only redirect targets an IPv6 literal.
///
/// # Errors
/// Represents a redirect rejected because its target uses an IPv6 literal.
#[derive(Debug)]
pub(crate) struct RedirectIpv6Violation;

impl fmt::Display for RedirectIpv6Violation {
    /// Formats the rejected IPv6 redirect reason.
    ///
    /// # Parameters
    /// - `formatter`: Destination formatter.
    ///
    /// # Returns
    /// Result from writing the rejection message.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("redirect target is IPv6 while ipv4_only=true")
    }
}

impl Error for RedirectIpv6Violation {}
