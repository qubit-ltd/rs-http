// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_sanitize::{
    SensitiveFields,
    SensitivityLevel,
};

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
}

impl LogSanitizePolicy {
    /// Creates a policy without built-in sensitive names.
    ///
    /// This constructor is intended for custom-only trace logging. Debug
    /// sanitization still merges built-in defaults back into the supplied
    /// policy before rendering diagnostic values.
    ///
    /// # Returns
    /// An empty log sanitization policy.
    #[inline(always)]
    pub fn empty() -> Self {
        Self {
            sensitive_headers: SensitiveFields::new(),
            sensitive_query_params: SensitiveFields::new(),
            sensitive_body_fields: SensitiveFields::new(),
        }
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
        self.sensitive_headers.extend_strongest(names, level);
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
        self.sensitive_headers.insert(name, level);
    }

    /// Removes one sensitive HTTP header.
    ///
    /// # Parameters
    /// - `name`: Header name to remove.
    ///
    /// # Returns
    /// Removed level, or `None` when the name was not configured.
    #[inline(always)]
    pub fn remove_sensitive_header(
        &mut self,
        name: &str,
    ) -> Option<SensitivityLevel> {
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
        self.sensitive_query_params.extend_strongest(names, level);
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
        self.sensitive_query_params.insert(name, level);
    }

    /// Removes one sensitive query parameter.
    ///
    /// # Parameters
    /// - `name`: Query parameter name to remove.
    ///
    /// # Returns
    /// Removed level, or `None` when the name was not configured.
    #[inline(always)]
    pub fn remove_sensitive_query_param(
        &mut self,
        name: &str,
    ) -> Option<SensitivityLevel> {
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
        self.sensitive_body_fields.extend_strongest(names, level);
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
        self.sensitive_body_fields.insert(name, level);
    }

    /// Removes one sensitive body field.
    ///
    /// # Parameters
    /// - `name`: Structured body field name to remove.
    ///
    /// # Returns
    /// Removed level, or `None` when the name was not configured.
    #[inline(always)]
    pub fn remove_sensitive_body_field(
        &mut self,
        name: &str,
    ) -> Option<SensitivityLevel> {
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
}

impl Default for LogSanitizePolicy {
    /// Creates a policy with built-in sensitive header, query, and body names.
    fn default() -> Self {
        Self {
            sensitive_headers: SensitiveFields::default(),
            sensitive_query_params: SensitiveFields::default(),
            sensitive_body_fields: SensitiveFields::default(),
        }
    }
}
