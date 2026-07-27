// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Builder for immutable HTTP log policies.

use qubit_redact::{
    http::{
        BodyBudget, DiagnosticBudget, HttpRedactionPolicy, HttpRedactionPolicyBuilder,
        TextBodyPolicy, UnkeyedJsonValuePolicy, UrlPathPolicy,
    },
    PolicyError, Sensitivity,
};

use super::LogRedactionPolicy;

/// Builds independent header, query, and body policy snapshots.
#[must_use]
#[derive(Debug, Clone)]
pub struct LogRedactionPolicyBuilder {
    /// Canonical HTTP policy construction state.
    http: HttpRedactionPolicyBuilder,
}

macro_rules! field_builder_methods {
    ($raise:ident, $override:ident, $allow_exact:ident, $allow_suffix:ident) => {
        /// Raises the named field to at least `level`.
        ///
        /// # Parameters
        ///
        /// * `name` - Field name to canonicalize.
        /// * `level` - Minimum sensitivity level.
        ///
        /// # Returns
        ///
        /// The updated builder.
        #[inline]
        pub fn $raise(mut self, name: &str, level: Sensitivity) -> Self {
            self.http = self.http.$raise(name, level);
            self
        }

        /// Replaces the named field's sensitivity with `level`.
        ///
        /// # Parameters
        ///
        /// * `name` - Field name to canonicalize.
        /// * `level` - Explicit replacement sensitivity level.
        ///
        /// # Returns
        ///
        /// The updated builder.
        #[inline]
        pub fn $override(mut self, name: &str, level: Sensitivity) -> Self {
            self.http = self.http.$override(name, level);
            self
        }

        /// Allows the exact field name to remain visible.
        ///
        /// # Parameters
        ///
        /// * `name` - Exact field name to allow after canonicalization.
        ///
        /// # Returns
        ///
        /// The updated builder.
        #[inline]
        pub fn $allow_exact(mut self, name: &str) -> Self {
            self.http = self.http.$allow_exact(name);
            self
        }

        /// Allows the field at token-suffix boundaries.
        ///
        /// # Parameters
        ///
        /// * `name` - Field suffix to allow after canonicalization.
        ///
        /// # Returns
        ///
        /// The updated builder.
        #[inline]
        pub fn $allow_suffix(mut self, name: &str) -> Self {
            self.http = self.http.$allow_suffix(name);
            self
        }
    };
}

impl LogRedactionPolicyBuilder {
    /// Creates a wrapper around the canonical HTTP policy builder.
    ///
    /// # Returns
    ///
    /// A builder using fail-closed runtime behavior defaults.
    #[inline]
    pub fn new() -> Self {
        Self {
            http: HttpRedactionPolicy::builder(),
        }
    }

    field_builder_methods!(
        raise_header,
        override_header,
        allow_header_exact,
        allow_header_suffix
    );
    field_builder_methods!(
        raise_query,
        override_query,
        allow_query_exact,
        allow_query_suffix
    );
    field_builder_methods!(
        raise_body,
        override_body,
        allow_body_exact,
        allow_body_suffix
    );

    /// Selects how non-root URL paths are rendered.
    ///
    /// # Parameters
    ///
    /// * `policy` - URL path visibility behavior.
    ///
    /// # Returns
    ///
    /// The updated builder.
    #[inline(always)]
    pub fn url_path_policy(mut self, policy: UrlPathPolicy) -> Self {
        self.http = self.http.url_path_policy(policy);
        self
    }

    /// Selects how opaque UTF-8 text bodies are rendered.
    ///
    /// # Parameters
    ///
    /// * `policy` - Opaque text visibility behavior.
    ///
    /// # Returns
    ///
    /// The updated builder.
    #[inline(always)]
    pub fn text_body_policy(mut self, policy: TextBodyPolicy) -> Self {
        self.http = self.http.text_body_policy(policy);
        self
    }

    /// Selects how JSON scalar values without field names are rendered.
    ///
    /// # Parameters
    ///
    /// * `policy` - Unkeyed scalar visibility behavior.
    ///
    /// # Returns
    ///
    /// The updated builder.
    #[inline(always)]
    pub fn unkeyed_json_value_policy(mut self, policy: UnkeyedJsonValuePolicy) -> Self {
        self.http = self.http.unkeyed_json_value_policy(policy);
        self
    }

    /// Sets checked hard body input and output limits.
    ///
    /// # Parameters
    ///
    /// * `budget` - Previously validated hard byte limits.
    ///
    /// # Returns
    ///
    /// The updated builder.
    #[inline(always)]
    pub fn body_budget(mut self, budget: BodyBudget) -> Self {
        self.http = self.http.body_budget(budget);
        self
    }

    /// Sets checked hard diagnostic input and output limits.
    ///
    /// # Parameters
    ///
    /// * `budget` - Previously validated hard byte limits.
    ///
    /// # Returns
    ///
    /// The updated builder.
    #[inline(always)]
    pub fn diagnostic_budget(mut self, budget: DiagnosticBudget) -> Self {
        self.http = self.http.diagnostic_budget(budget);
        self
    }

    /// Validates all field rules and builds one immutable policy snapshot.
    ///
    /// # Returns
    ///
    /// The complete log policy when every field rule is valid.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError`] from the first invalid header, query, or body
    /// policy encountered in that order.
    pub fn build(self) -> Result<LogRedactionPolicy, PolicyError> {
        self.http.build().map(LogRedactionPolicy::new)
    }
}

impl Default for LogRedactionPolicyBuilder {
    /// Creates the same construction state as [`Self::new`].
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}
