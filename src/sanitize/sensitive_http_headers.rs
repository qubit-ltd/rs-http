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

/// Canonical set of HTTP header names whose values should be masked in logs.
///
/// Matching is case-insensitive and uses the same canonical field-name rules as
/// `qubit-sanitize`, including removal of common separators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensitiveHttpHeaders {
    /// Shared sensitive field set used by `qubit-sanitize`.
    fields: SensitiveFields,
}

impl SensitiveHttpHeaders {
    /// Creates an empty set without built-in names.
    ///
    /// # Returns
    /// Empty [`SensitiveHttpHeaders`].
    pub fn new() -> Self {
        Self {
            fields: SensitiveFields::new(),
        }
    }

    /// Returns whether `header_name` is treated as sensitive.
    ///
    /// Matching follows log sanitization semantics: exact canonical names and
    /// contextual suffixes are both accepted.
    ///
    /// # Parameters
    /// - `header_name`: Header name to test.
    ///
    /// # Returns
    /// `true` if values for this header should be masked.
    pub fn contains(&self, header_name: &str) -> bool {
        self.field_sanitizer()
            .sensitivity_for_name(header_name, NameMatchMode::ExactOrSuffix)
            .is_some()
    }

    /// Inserts one header name after canonicalizing it.
    ///
    /// # Parameters
    /// - `header_name`: Header name to mark sensitive.
    pub fn insert(&mut self, header_name: &str) {
        self.fields.insert(header_name, SensitivityLevel::High);
    }

    /// Inserts each header from `headers`.
    ///
    /// # Parameters
    /// - `headers`: Header names to mark sensitive.
    pub fn extend<I, S>(&mut self, headers: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for header in headers {
            self.insert(header.as_ref());
        }
    }

    /// Clears all sensitive header names.
    pub fn clear(&mut self) {
        self.fields = SensitiveFields::new();
    }

    /// Returns the number of stored header names.
    ///
    /// # Returns
    /// Stored header count.
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Returns whether no header names are stored.
    ///
    /// # Returns
    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Iterates canonical header names.
    ///
    /// # Returns
    /// Iterator over stored canonical header names.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.fields.iter().map(|(field, _)| field)
    }

    /// Extends this set with another header set, preserving sensitivity levels.
    ///
    /// # Parameters
    /// - `other`: Source header set.
    pub(crate) fn extend_from(&mut self, other: &Self) {
        for (field, level) in other.fields.iter() {
            self.fields.insert(field, level);
        }
    }

    /// Creates a core field sanitizer from this header set.
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

impl Default for SensitiveHttpHeaders {
    /// Creates a set containing built-in sensitive header names.
    fn default() -> Self {
        Self {
            fields: SensitiveFields::default(),
        }
    }
}
