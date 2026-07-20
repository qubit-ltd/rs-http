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
        BodyBudget,
        HttpRedactionPolicy,
        TextBodyPolicy,
        UnkeyedJsonValuePolicy,
        UrlPathPolicy,
    },
    PolicyError,
    RedactionPolicyBuilder,
    Sensitivity,
};

use super::LogRedactionPolicy;

/// Builds independent header, query, and body policy snapshots.
#[must_use]
#[derive(Debug, Clone)]
pub struct LogRedactionPolicyBuilder {
    /// Header-field policy construction state.
    header: RedactionPolicyBuilder,
    /// Query and form field-policy construction state.
    query: RedactionPolicyBuilder,
    /// Structured-body field-policy construction state.
    body: RedactionPolicyBuilder,
    /// URL path visibility choice.
    url_path_policy: UrlPathPolicy,
    /// Opaque text body visibility choice.
    text_body_policy: TextBodyPolicy,
    /// Unkeyed JSON scalar visibility choice.
    unkeyed_json_value_policy: UnkeyedJsonValuePolicy,
    /// Hard body input and output limits.
    body_budget: BodyBudget,
}

macro_rules! field_builder_methods {
    ($raise:ident, $override:ident, $allow_exact:ident, $allow_suffix:ident, $field:ident) => {
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
            self.$field = self.$field.raise(name, level);
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
            self.$field = self.$field.override_level(name, level);
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
            self.$field = self.$field.allow_exact(name);
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
            self.$field = self.$field.allow_suffix(name);
            self
        }
    };
}

impl LogRedactionPolicyBuilder {
    /// Creates a builder from three independent default policy builders.
    ///
    /// # Returns
    ///
    /// A builder using fail-closed runtime behavior defaults.
    #[inline]
    pub fn new() -> Self {
        Self {
            header: RedactionPolicyBuilder::new(),
            query: RedactionPolicyBuilder::new(),
            body: RedactionPolicyBuilder::new(),
            url_path_policy: UrlPathPolicy::default(),
            text_body_policy: TextBodyPolicy::default(),
            unkeyed_json_value_policy: UnkeyedJsonValuePolicy::default(),
            body_budget: BodyBudget::default(),
        }
    }

    field_builder_methods!(
        raise_header,
        override_header,
        allow_header_exact,
        allow_header_suffix,
        header
    );
    field_builder_methods!(
        raise_query,
        override_query,
        allow_query_exact,
        allow_query_suffix,
        query
    );
    field_builder_methods!(
        raise_body,
        override_body,
        allow_body_exact,
        allow_body_suffix,
        body
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
    pub const fn url_path_policy(mut self, policy: UrlPathPolicy) -> Self {
        self.url_path_policy = policy;
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
    pub const fn text_body_policy(mut self, policy: TextBodyPolicy) -> Self {
        self.text_body_policy = policy;
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
    pub const fn unkeyed_json_value_policy(
        mut self,
        policy: UnkeyedJsonValuePolicy,
    ) -> Self {
        self.unkeyed_json_value_policy = policy;
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
    pub const fn body_budget(mut self, budget: BodyBudget) -> Self {
        self.body_budget = budget;
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
        let header = self.header.build()?;
        let query = self.query.build()?;
        let body = self.body.build()?;
        let http_policy = HttpRedactionPolicy::builder(header)
            .query_policy(query)
            .body_policy(body)
            .url_path_policy(self.url_path_policy)
            .text_body_policy(self.text_body_policy)
            .unkeyed_json_value_policy(self.unkeyed_json_value_policy)
            .body_budget(self.body_budget)
            .build();
        Ok(LogRedactionPolicy::new(http_policy))
    }
}

impl Default for LogRedactionPolicyBuilder {
    /// Creates the same construction state as [`Self::new`].
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}
