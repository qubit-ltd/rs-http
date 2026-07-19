// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_sanitize::{
    FieldSanitizePolicy,
    SensitivityLevel,
    TextBodyPolicy,
    UrlPathPolicy,
};

/// Policy used by [`LogSanitizer`](super::LogSanitizer) to mask sensitive log
/// data.
///
/// Each HTTP logging domain owns a complete [`FieldSanitizePolicy`], including
/// sensitive names, explicit exclusions, and mask policies. Domain-specific
/// methods keep the public API independent of that internal composition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogSanitizePolicy {
    /// Field sanitization policy for HTTP headers.
    header_policy: FieldSanitizePolicy,
    /// Field sanitization policy for URL query parameters.
    query_param_policy: FieldSanitizePolicy,
    /// Field sanitization policy for JSON, form, and multipart body fields.
    body_field_policy: FieldSanitizePolicy,
    /// Rendering policy for opaque HTTP text bodies.
    text_body_policy: TextBodyPolicy,
    /// Rendering policy for complete URL paths.
    url_path_policy: UrlPathPolicy,
}

impl LogSanitizePolicy {
    /// Creates a policy without built-in sensitive names while retaining
    /// default opaque-text and URL-path redaction.
    ///
    /// This constructor is intended for custom-only trace logging. Debug
    /// sanitization merges built-in defaults back into the supplied policy
    /// before rendering diagnostic values, except for names explicitly
    /// removed from this policy.
    ///
    /// # Returns
    ///
    /// An empty log sanitization policy.
    #[inline]
    pub fn empty() -> Self {
        Self {
            header_policy: FieldSanitizePolicy::empty(),
            query_param_policy: FieldSanitizePolicy::empty(),
            body_field_policy: FieldSanitizePolicy::empty(),
            text_body_policy: TextBodyPolicy::Redact,
            url_path_policy: UrlPathPolicy::Redact,
        }
    }

    /// Returns this policy with a replacement URL path policy.
    ///
    /// Selecting [`UrlPathPolicy::Preserve`] can expose secret path segments
    /// and should be an explicit diagnostic decision.
    ///
    /// # Parameters
    ///
    /// * `policy` - New rendering policy for complete URL paths.
    ///
    /// # Returns
    ///
    /// The updated log sanitization policy.
    #[must_use]
    #[inline]
    pub fn with_url_path_policy(mut self, policy: UrlPathPolicy) -> Self {
        self.url_path_policy = policy;
        self
    }

    /// Returns this policy with a replacement opaque-text body policy.
    ///
    /// Passing opaque text through can expose secrets that have no field name
    /// for structured matching; callers must opt into that boundary.
    ///
    /// # Parameters
    ///
    /// * `policy` - New policy for declared `text/*` bodies and non-sensitive
    ///   multipart text parts.
    ///
    /// # Returns
    ///
    /// The updated log sanitization policy.
    #[must_use]
    #[inline]
    pub fn with_text_body_policy(mut self, policy: TextBodyPolicy) -> Self {
        self.text_body_policy = policy;
        self
    }

    /// Returns the policy used for complete URL paths.
    ///
    /// # Returns
    ///
    /// The current URL path policy. Both [`Self::empty`] and
    /// [`Self::default`] use [`UrlPathPolicy::Redact`].
    #[inline(always)]
    pub const fn url_path_policy(&self) -> UrlPathPolicy {
        self.url_path_policy
    }

    /// Replaces the policy used for complete URL paths.
    ///
    /// # Parameters
    ///
    /// * `policy` - New rendering policy for complete URL paths.
    #[inline(always)]
    pub fn set_url_path_policy(&mut self, policy: UrlPathPolicy) {
        self.url_path_policy = policy;
    }

    /// Returns the policy used for opaque HTTP text bodies.
    ///
    /// # Returns
    ///
    /// The current text body policy. Both [`Self::empty`] and
    /// [`Self::default`] use [`TextBodyPolicy::Redact`].
    #[inline(always)]
    pub const fn text_body_policy(&self) -> TextBodyPolicy {
        self.text_body_policy
    }

    /// Replaces the policy used for opaque HTTP text bodies.
    ///
    /// # Parameters
    ///
    /// * `policy` - New policy for declared `text/*` bodies and non-sensitive
    ///   multipart text parts.
    #[inline(always)]
    pub fn set_text_body_policy(&mut self, policy: TextBodyPolicy) {
        self.text_body_policy = policy;
    }

    /// Returns the sensitivity level configured for an HTTP header.
    ///
    /// # Parameters
    ///
    /// * `name` - Header name to resolve.
    ///
    /// # Returns
    ///
    /// Configured sensitivity level, or `None` when the name is not sensitive.
    #[inline(always)]
    pub fn sensitivity_for_header(
        &self,
        name: &str,
    ) -> Option<SensitivityLevel> {
        self.header_policy.sensitive_fields().level_for(name)
    }

    /// Adds a sensitive HTTP header without lowering an existing level.
    ///
    /// # Parameters
    ///
    /// * `name` - Header name to mark sensitive.
    /// * `level` - Minimum sensitivity level for the header.
    #[inline(always)]
    pub fn insert_sensitive_header(
        &mut self,
        name: &str,
        level: SensitivityLevel,
    ) {
        self.header_policy.insert_sensitive_field(name, level);
    }

    /// Adds sensitive HTTP headers without lowering existing levels.
    ///
    /// # Parameters
    ///
    /// * `names` - Header names to mark sensitive.
    /// * `level` - Minimum sensitivity level for every header.
    #[inline(always)]
    pub fn extend_sensitive_headers<I, S>(
        &mut self,
        names: I,
        level: SensitivityLevel,
    ) where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.header_policy.extend_sensitive_fields(names, level);
    }

    /// Explicitly replaces one sensitive HTTP header level.
    ///
    /// # Parameters
    ///
    /// * `name` - Header name whose level should be replaced.
    /// * `level` - Replacement level, even when weaker than the current level.
    #[inline(always)]
    pub fn set_sensitive_header_level(
        &mut self,
        name: &str,
        level: SensitivityLevel,
    ) {
        self.header_policy.set_sensitive_field_level(name, level);
    }

    /// Removes one sensitive HTTP header.
    ///
    /// Removing a built-in name is an explicit disclosure decision: matching
    /// unmarked header values may appear unchanged in logs and diagnostic
    /// output. A value marked with [`http::HeaderValue::set_sensitive`]
    /// remains a value-level `Secret` declaration and is still masked.
    ///
    /// # Parameters
    ///
    /// * `name` - Header name to remove.
    ///
    /// # Returns
    ///
    /// Removed level, or `None` when the name was not configured. Either case
    /// records an explicit exclusion for debug sanitization.
    #[inline(always)]
    pub fn remove_sensitive_header(
        &mut self,
        name: &str,
    ) -> Option<SensitivityLevel> {
        self.header_policy.exclude_sensitive_field(name)
    }

    /// Returns the sensitivity level configured for a URL query parameter.
    ///
    /// # Parameters
    ///
    /// * `name` - Query parameter name to resolve.
    ///
    /// # Returns
    ///
    /// Configured sensitivity level, or `None` when the name is not sensitive.
    #[inline(always)]
    pub fn sensitivity_for_query_param(
        &self,
        name: &str,
    ) -> Option<SensitivityLevel> {
        self.query_param_policy.sensitive_fields().level_for(name)
    }

    /// Adds a sensitive query parameter without lowering an existing level.
    ///
    /// # Parameters
    ///
    /// * `name` - Query parameter name to mark sensitive.
    /// * `level` - Minimum sensitivity level for the parameter.
    #[inline(always)]
    pub fn insert_sensitive_query_param(
        &mut self,
        name: &str,
        level: SensitivityLevel,
    ) {
        self.query_param_policy.insert_sensitive_field(name, level);
    }

    /// Adds sensitive query parameters without lowering existing levels.
    ///
    /// # Parameters
    ///
    /// * `names` - Query parameter names to mark sensitive.
    /// * `level` - Minimum sensitivity level for every parameter.
    #[inline(always)]
    pub fn extend_sensitive_query_params<I, S>(
        &mut self,
        names: I,
        level: SensitivityLevel,
    ) where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.query_param_policy
            .extend_sensitive_fields(names, level);
    }

    /// Explicitly replaces one sensitive query parameter level.
    ///
    /// # Parameters
    ///
    /// * `name` - Query parameter name whose level should be replaced.
    /// * `level` - Replacement level, even when weaker than the current level.
    #[inline(always)]
    pub fn set_sensitive_query_param_level(
        &mut self,
        name: &str,
        level: SensitivityLevel,
    ) {
        self.query_param_policy
            .set_sensitive_field_level(name, level);
    }

    /// Removes one sensitive query parameter.
    ///
    /// Removing a built-in name is an explicit disclosure decision: matching
    /// query values may appear unchanged in logs and diagnostic output.
    ///
    /// # Parameters
    ///
    /// * `name` - Query parameter name to remove.
    ///
    /// # Returns
    ///
    /// Removed level, or `None` when the name was not configured. Either case
    /// records an explicit exclusion for debug sanitization.
    #[inline(always)]
    pub fn remove_sensitive_query_param(
        &mut self,
        name: &str,
    ) -> Option<SensitivityLevel> {
        self.query_param_policy.exclude_sensitive_field(name)
    }

    /// Returns the sensitivity level configured for a structured body field.
    ///
    /// # Parameters
    ///
    /// * `name` - Body field name to resolve.
    ///
    /// # Returns
    ///
    /// Configured sensitivity level, or `None` when the name is not sensitive.
    #[inline(always)]
    pub fn sensitivity_for_body_field(
        &self,
        name: &str,
    ) -> Option<SensitivityLevel> {
        self.body_field_policy.sensitive_fields().level_for(name)
    }

    /// Adds a sensitive body field without lowering an existing level.
    ///
    /// # Parameters
    ///
    /// * `name` - Structured body field name to mark sensitive.
    /// * `level` - Minimum sensitivity level for the field.
    #[inline(always)]
    pub fn insert_sensitive_body_field(
        &mut self,
        name: &str,
        level: SensitivityLevel,
    ) {
        self.body_field_policy.insert_sensitive_field(name, level);
    }

    /// Adds sensitive body fields without lowering existing levels.
    ///
    /// # Parameters
    ///
    /// * `names` - Structured body field names to mark sensitive.
    /// * `level` - Minimum sensitivity level for every field.
    #[inline(always)]
    pub fn extend_sensitive_body_fields<I, S>(
        &mut self,
        names: I,
        level: SensitivityLevel,
    ) where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.body_field_policy.extend_sensitive_fields(names, level);
    }

    /// Explicitly replaces one sensitive body field level.
    ///
    /// # Parameters
    ///
    /// * `name` - Body field name whose level should be replaced.
    /// * `level` - Replacement level, even when weaker than the current level.
    #[inline(always)]
    pub fn set_sensitive_body_field_level(
        &mut self,
        name: &str,
        level: SensitivityLevel,
    ) {
        self.body_field_policy
            .set_sensitive_field_level(name, level);
    }

    /// Removes one sensitive body field.
    ///
    /// Removing a built-in name is an explicit disclosure decision: matching
    /// body values may appear unchanged in logs and diagnostic output.
    ///
    /// # Parameters
    ///
    /// * `name` - Structured body field name to remove.
    ///
    /// # Returns
    ///
    /// Removed level, or `None` when the name was not configured. Either case
    /// records an explicit exclusion for debug sanitization.
    #[inline(always)]
    pub fn remove_sensitive_body_field(
        &mut self,
        name: &str,
    ) -> Option<SensitivityLevel> {
        self.body_field_policy.exclude_sensitive_field(name)
    }

    /// Returns the header field policy for internal adapters.
    ///
    /// # Returns
    ///
    /// Borrowed policy containing header fields, exclusions, and masks.
    #[inline(always)]
    pub(crate) const fn header_policy(&self) -> &FieldSanitizePolicy {
        &self.header_policy
    }

    /// Returns the query-parameter field policy for internal adapters.
    ///
    /// # Returns
    ///
    /// Borrowed policy containing query fields, exclusions, and masks.
    #[inline(always)]
    pub(crate) const fn query_param_policy(&self) -> &FieldSanitizePolicy {
        &self.query_param_policy
    }

    /// Returns the body field policy for internal adapters.
    ///
    /// # Returns
    ///
    /// Borrowed policy containing body fields, exclusions, and masks.
    #[inline(always)]
    pub(crate) const fn body_field_policy(&self) -> &FieldSanitizePolicy {
        &self.body_field_policy
    }

    /// Applies explicit exclusions to another policy.
    ///
    /// # Parameters
    ///
    /// * `policy` - Policy from which explicitly excluded names are removed.
    pub(crate) fn apply_exclusions_to(&self, policy: &mut Self) {
        for name in self.header_policy.excluded_sensitive_fields() {
            policy.remove_sensitive_header(name);
        }
        for name in self.query_param_policy.excluded_sensitive_fields() {
            policy.remove_sensitive_query_param(name);
        }
        for name in self.body_field_policy.excluded_sensitive_fields() {
            policy.remove_sensitive_body_field(name);
        }
    }
}

impl Default for LogSanitizePolicy {
    /// Creates a policy with built-in sensitive names, opaque-text redaction,
    /// and URL-path redaction.
    ///
    /// # Returns
    ///
    /// A default policy that redacts opaque text and URL paths.
    #[inline]
    fn default() -> Self {
        Self {
            header_policy: FieldSanitizePolicy::default(),
            query_param_policy: FieldSanitizePolicy::default(),
            body_field_policy: FieldSanitizePolicy::default(),
            text_body_policy: TextBodyPolicy::Redact,
            url_path_policy: UrlPathPolicy::Redact,
        }
    }
}
