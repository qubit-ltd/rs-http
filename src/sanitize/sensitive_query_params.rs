/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use qubit_sanitize::{
    FieldSanitizePolicy,
    FieldSanitizer,
    MaskPolicies,
    NameMatchMode,
    SensitiveFields,
    SensitivityLevel,
};

use super::{
    default_sensitive_names::default_sensitive_fields,
    DEFAULT_SENSITIVE_QUERY_PARAM_NAMES,
};

/// Set of query parameter names whose values should be masked.
///
/// Names use the same canonical field-name rules as `qubit-sanitize`, so
/// `access_token`, `access-token`, and `accessToken` are equivalent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensitiveQueryParams {
    /// Shared sensitive field set used by `qubit-sanitize`.
    fields: SensitiveFields,
}

impl SensitiveQueryParams {
    /// Creates an empty set.
    ///
    /// # Returns
    /// Empty [`SensitiveQueryParams`].
    pub fn new() -> Self {
        Self {
            fields: SensitiveFields::new(),
        }
    }

    /// Returns whether `name` is sensitive.
    ///
    /// Matching follows log sanitization semantics: exact canonical names and
    /// contextual suffixes are both accepted.
    ///
    /// # Parameters
    /// - `name`: Query parameter name.
    ///
    /// # Returns
    /// `true` if the value should be masked in logged URLs.
    pub fn contains(&self, name: &str) -> bool {
        self.field_sanitizer()
            .sensitivity_for_name(name, NameMatchMode::ExactOrSuffix)
            .is_some()
    }

    /// Inserts one query parameter name.
    ///
    /// # Parameters
    /// - `name`: Query parameter name to mark sensitive.
    pub fn insert(&mut self, name: &str) {
        self.fields.insert(name, SensitivityLevel::High);
    }

    /// Inserts many query parameter names.
    ///
    /// # Parameters
    /// - `names`: Query parameter names to mark sensitive.
    pub fn extend<I, S>(&mut self, names: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for name in names {
            self.insert(name.as_ref());
        }
    }

    /// Clears all names.
    pub fn clear(&mut self) {
        self.fields = SensitiveFields::new();
    }

    /// Returns the number of stored query parameter names.
    ///
    /// # Returns
    /// Stored query parameter count.
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Returns whether no query parameter names are stored.
    ///
    /// # Returns
    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Iterates canonical query parameter names.
    ///
    /// # Returns
    /// Iterator over stored canonical query parameter names.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.fields.iter().map(|(field, _)| field)
    }

    /// Extends this set with another query set, preserving sensitivity levels.
    ///
    /// # Parameters
    /// - `other`: Source query set.
    pub(crate) fn extend_from(&mut self, other: &Self) {
        for (field, level) in other.fields.iter() {
            self.fields.insert(field, level);
        }
    }

    /// Creates a core field sanitizer from this query set.
    ///
    /// # Returns
    /// Field sanitizer using shared `qubit-sanitize` masking rules.
    pub(crate) fn field_sanitizer(&self) -> FieldSanitizer {
        FieldSanitizer::new(FieldSanitizePolicy {
            sensitive_fields: self.fields.clone(),
            mask_policies: MaskPolicies::default(),
        })
    }
}

impl Default for SensitiveQueryParams {
    /// Creates a set containing common token-like query parameter names.
    fn default() -> Self {
        Self {
            fields: default_sensitive_fields(DEFAULT_SENSITIVE_QUERY_PARAM_NAMES),
        }
    }
}
