// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::BTreeSet;

use qubit_sanitize::{
    canonicalize_field_name,
    SensitiveFields,
    SensitivityLevel,
    TextBodyPolicy,
};

use super::UrlPathPolicy;

/// Policy used by [`LogSanitizer`](super::LogSanitizer) to mask sensitive log
/// data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogSanitizePolicy {
    /// Sensitive HTTP header names.
    sensitive_headers: SensitiveFields,
    /// Sensitive URL query parameter names.
    sensitive_query_params: SensitiveFields,
    /// Sensitive JSON/form/multipart body field names.
    sensitive_body_fields: SensitiveFields,
    /// Canonical header names explicitly excluded from built-in defaults.
    excluded_sensitive_headers: BTreeSet<String>,
    /// Canonical query names explicitly excluded from built-in defaults.
    excluded_sensitive_query_params: BTreeSet<String>,
    /// Canonical body names explicitly excluded from built-in defaults.
    excluded_sensitive_body_fields: BTreeSet<String>,
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
    /// An empty log sanitization policy.
    #[inline(always)]
    pub fn empty() -> Self {
        Self {
            sensitive_headers: SensitiveFields::new(),
            sensitive_query_params: SensitiveFields::new(),
            sensitive_body_fields: SensitiveFields::new(),
            excluded_sensitive_headers: BTreeSet::new(),
            excluded_sensitive_query_params: BTreeSet::new(),
            excluded_sensitive_body_fields: BTreeSet::new(),
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
    /// - `name`: Header name to resolve.
    ///
    /// # Returns
    /// Configured sensitivity level, or `None` when the name is not sensitive.
    #[inline(always)]
    pub fn sensitivity_for_header(
        &self,
        name: &str,
    ) -> Option<SensitivityLevel> {
        self.sensitive_headers.level_for(name)
    }

    /// Adds a sensitive HTTP header without lowering an existing level.
    ///
    /// # Parameters
    /// - `name`: Header name to mark sensitive.
    /// - `level`: Minimum sensitivity level for the header.
    #[inline(always)]
    pub fn insert_sensitive_header(
        &mut self,
        name: &str,
        level: SensitivityLevel,
    ) {
        self.excluded_sensitive_headers
            .remove(&canonicalize_field_name(name));
        self.sensitive_headers.insert_strongest(name, level);
    }

    /// Adds sensitive HTTP headers without lowering existing levels.
    ///
    /// # Parameters
    /// - `names`: Header names to mark sensitive.
    /// - `level`: Minimum sensitivity level for every header.
    #[inline(always)]
    pub fn extend_sensitive_headers<I, S>(
        &mut self,
        names: I,
        level: SensitivityLevel,
    ) where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for name in names {
            self.insert_sensitive_header(name.as_ref(), level);
        }
    }

    /// Explicitly replaces one sensitive HTTP header level.
    ///
    /// # Parameters
    /// - `name`: Header name whose level should be replaced.
    /// - `level`: Replacement level, even when weaker than the current level.
    #[inline(always)]
    pub fn set_sensitive_header_level(
        &mut self,
        name: &str,
        level: SensitivityLevel,
    ) {
        self.excluded_sensitive_headers
            .remove(&canonicalize_field_name(name));
        self.sensitive_headers.insert(name, level);
    }

    /// Removes one sensitive HTTP header.
    ///
    /// Removing a built-in name is an explicit disclosure decision: matching
    /// header values may appear unchanged in logs and diagnostic output.
    ///
    /// # Parameters
    /// - `name`: Header name to remove.
    ///
    /// # Returns
    /// Removed level, or `None` when the name was not configured. Either case
    /// records an explicit exclusion for debug sanitization.
    #[inline(always)]
    pub fn remove_sensitive_header(
        &mut self,
        name: &str,
    ) -> Option<SensitivityLevel> {
        let canonical = canonicalize_field_name(name);
        if !canonical.is_empty() {
            self.excluded_sensitive_headers.insert(canonical);
        }
        self.sensitive_headers.remove(name)
    }

    /// Returns the sensitivity level configured for a URL query parameter.
    ///
    /// # Parameters
    /// - `name`: Query parameter name to resolve.
    ///
    /// # Returns
    /// Configured sensitivity level, or `None` when the name is not sensitive.
    #[inline(always)]
    pub fn sensitivity_for_query_param(
        &self,
        name: &str,
    ) -> Option<SensitivityLevel> {
        self.sensitive_query_params.level_for(name)
    }

    /// Adds a sensitive query parameter without lowering an existing level.
    ///
    /// # Parameters
    /// - `name`: Query parameter name to mark sensitive.
    /// - `level`: Minimum sensitivity level for the parameter.
    #[inline(always)]
    pub fn insert_sensitive_query_param(
        &mut self,
        name: &str,
        level: SensitivityLevel,
    ) {
        self.excluded_sensitive_query_params
            .remove(&canonicalize_field_name(name));
        self.sensitive_query_params.insert_strongest(name, level);
    }

    /// Adds sensitive query parameters without lowering existing levels.
    ///
    /// # Parameters
    /// - `names`: Query parameter names to mark sensitive.
    /// - `level`: Minimum sensitivity level for every parameter.
    #[inline(always)]
    pub fn extend_sensitive_query_params<I, S>(
        &mut self,
        names: I,
        level: SensitivityLevel,
    ) where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for name in names {
            self.insert_sensitive_query_param(name.as_ref(), level);
        }
    }

    /// Explicitly replaces one sensitive query parameter level.
    ///
    /// # Parameters
    /// - `name`: Query parameter name whose level should be replaced.
    /// - `level`: Replacement level, even when weaker than the current level.
    #[inline(always)]
    pub fn set_sensitive_query_param_level(
        &mut self,
        name: &str,
        level: SensitivityLevel,
    ) {
        self.excluded_sensitive_query_params
            .remove(&canonicalize_field_name(name));
        self.sensitive_query_params.insert(name, level);
    }

    /// Removes one sensitive query parameter.
    ///
    /// Removing a built-in name is an explicit disclosure decision: matching
    /// query values may appear unchanged in logs and diagnostic output.
    ///
    /// # Parameters
    /// - `name`: Query parameter name to remove.
    ///
    /// # Returns
    /// Removed level, or `None` when the name was not configured. Either case
    /// records an explicit exclusion for debug sanitization.
    #[inline(always)]
    pub fn remove_sensitive_query_param(
        &mut self,
        name: &str,
    ) -> Option<SensitivityLevel> {
        let canonical = canonicalize_field_name(name);
        if !canonical.is_empty() {
            self.excluded_sensitive_query_params.insert(canonical);
        }
        self.sensitive_query_params.remove(name)
    }

    /// Returns the sensitivity level configured for a structured body field.
    ///
    /// # Parameters
    /// - `name`: Body field name to resolve.
    ///
    /// # Returns
    /// Configured sensitivity level, or `None` when the name is not sensitive.
    #[inline(always)]
    pub fn sensitivity_for_body_field(
        &self,
        name: &str,
    ) -> Option<SensitivityLevel> {
        self.sensitive_body_fields.level_for(name)
    }

    /// Adds a sensitive body field without lowering an existing level.
    ///
    /// # Parameters
    /// - `name`: Structured body field name to mark sensitive.
    /// - `level`: Minimum sensitivity level for the field.
    #[inline(always)]
    pub fn insert_sensitive_body_field(
        &mut self,
        name: &str,
        level: SensitivityLevel,
    ) {
        self.excluded_sensitive_body_fields
            .remove(&canonicalize_field_name(name));
        self.sensitive_body_fields.insert_strongest(name, level);
    }

    /// Adds sensitive body fields without lowering existing levels.
    ///
    /// # Parameters
    /// - `names`: Structured body field names to mark sensitive.
    /// - `level`: Minimum sensitivity level for every field.
    #[inline(always)]
    pub fn extend_sensitive_body_fields<I, S>(
        &mut self,
        names: I,
        level: SensitivityLevel,
    ) where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for name in names {
            self.insert_sensitive_body_field(name.as_ref(), level);
        }
    }

    /// Explicitly replaces one sensitive body field level.
    ///
    /// # Parameters
    /// - `name`: Body field name whose level should be replaced.
    /// - `level`: Replacement level, even when weaker than the current level.
    #[inline(always)]
    pub fn set_sensitive_body_field_level(
        &mut self,
        name: &str,
        level: SensitivityLevel,
    ) {
        self.excluded_sensitive_body_fields
            .remove(&canonicalize_field_name(name));
        self.sensitive_body_fields.insert(name, level);
    }

    /// Removes one sensitive body field.
    ///
    /// Removing a built-in name is an explicit disclosure decision: matching
    /// body values may appear unchanged in logs and diagnostic output.
    ///
    /// # Parameters
    /// - `name`: Structured body field name to remove.
    ///
    /// # Returns
    /// Removed level, or `None` when the name was not configured. Either case
    /// records an explicit exclusion for debug sanitization.
    #[inline(always)]
    pub fn remove_sensitive_body_field(
        &mut self,
        name: &str,
    ) -> Option<SensitivityLevel> {
        let canonical = canonicalize_field_name(name);
        if !canonical.is_empty() {
            self.excluded_sensitive_body_fields.insert(canonical);
        }
        self.sensitive_body_fields.remove(name)
    }

    /// Returns configured sensitive HTTP headers for internal adapters.
    ///
    /// # Returns
    /// Borrowed sensitive header set.
    #[inline(always)]
    pub(crate) const fn sensitive_headers(&self) -> &SensitiveFields {
        &self.sensitive_headers
    }

    /// Returns configured sensitive query parameters for internal adapters.
    ///
    /// # Returns
    /// Borrowed sensitive query parameter set.
    #[inline(always)]
    pub(crate) const fn sensitive_query_params(&self) -> &SensitiveFields {
        &self.sensitive_query_params
    }

    /// Returns configured sensitive body fields for internal adapters.
    ///
    /// # Returns
    /// Borrowed sensitive body field set.
    #[inline(always)]
    pub(crate) const fn sensitive_body_fields(&self) -> &SensitiveFields {
        &self.sensitive_body_fields
    }

    /// Returns canonical header names explicitly excluded from matching.
    ///
    /// # Returns
    /// Borrowed header exclusion set.
    #[inline(always)]
    pub(crate) const fn excluded_sensitive_headers(&self) -> &BTreeSet<String> {
        &self.excluded_sensitive_headers
    }

    /// Returns canonical query names explicitly excluded from matching.
    ///
    /// # Returns
    /// Borrowed query exclusion set.
    #[inline(always)]
    pub(crate) const fn excluded_sensitive_query_params(
        &self,
    ) -> &BTreeSet<String> {
        &self.excluded_sensitive_query_params
    }

    /// Returns canonical body names explicitly excluded from matching.
    ///
    /// # Returns
    /// Borrowed body exclusion set.
    #[inline(always)]
    pub(crate) const fn excluded_sensitive_body_fields(
        &self,
    ) -> &BTreeSet<String> {
        &self.excluded_sensitive_body_fields
    }

    /// Applies explicit exclusions to another policy.
    ///
    /// # Parameters
    ///
    /// * `policy` - Policy from which explicitly excluded names are removed.
    pub(crate) fn apply_exclusions_to(&self, policy: &mut Self) {
        for name in &self.excluded_sensitive_headers {
            policy.remove_sensitive_header(name);
        }
        for name in &self.excluded_sensitive_query_params {
            policy.remove_sensitive_query_param(name);
        }
        for name in &self.excluded_sensitive_body_fields {
            policy.remove_sensitive_body_field(name);
        }
    }
}

impl Default for LogSanitizePolicy {
    /// Creates a policy with built-in sensitive names, opaque-text redaction,
    /// and URL-path redaction.
    ///
    /// # Returns
    /// A default policy that redacts opaque text and URL paths.
    fn default() -> Self {
        Self {
            sensitive_headers: SensitiveFields::default(),
            sensitive_query_params: SensitiveFields::default(),
            sensitive_body_fields: SensitiveFields::default(),
            excluded_sensitive_headers: BTreeSet::new(),
            excluded_sensitive_query_params: BTreeSet::new(),
            excluded_sensitive_body_fields: BTreeSet::new(),
            text_body_policy: TextBodyPolicy::Redact,
            url_path_policy: UrlPathPolicy::Redact,
        }
    }
}
