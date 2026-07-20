// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Immutable policy snapshot for HTTP diagnostics.

use qubit_redact::http::HttpRedactionPolicy;

use super::LogRedactionPolicyBuilder;

/// Wraps the complete immutable runtime HTTP redaction policy.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRedactionPolicy {
    /// Policy shared by every diagnostic rendering path.
    http_policy: HttpRedactionPolicy,
}

impl LogRedactionPolicy {
    /// Creates a builder initialized from the current default redaction policy.
    ///
    /// # Returns
    ///
    /// A mutable builder that produces one immutable snapshot.
    #[inline(always)]
    pub fn builder() -> LogRedactionPolicyBuilder {
        LogRedactionPolicyBuilder::new()
    }

    /// Creates a log policy from a complete runtime HTTP policy.
    ///
    /// # Parameters
    ///
    /// * `http_policy` - Runtime policy to wrap.
    ///
    /// # Returns
    ///
    /// A log policy containing exactly that snapshot.
    #[inline(always)]
    pub(crate) const fn new(http_policy: HttpRedactionPolicy) -> Self {
        Self { http_policy }
    }

    /// Returns the complete runtime HTTP policy.
    ///
    /// # Returns
    ///
    /// The immutable policy used for URLs, headers, and bodies.
    #[inline(always)]
    pub const fn http_policy(&self) -> &HttpRedactionPolicy {
        &self.http_policy
    }
}

impl Default for LogRedactionPolicy {
    /// Wraps the runtime's current default HTTP policy.
    ///
    /// # Returns
    ///
    /// A fail-closed immutable log policy.
    #[inline(always)]
    fn default() -> Self {
        Self::new(HttpRedactionPolicy::default())
    }
}
