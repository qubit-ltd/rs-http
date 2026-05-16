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
    SensitiveFields,
    SensitivityLevel,
};

/// Set of structured body field names whose values should be masked.
///
/// Names use the same canonical field-name rules as `qubit-sanitize`, so
/// `client_secret`, `client-secret`, and `clientSecret` are equivalent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensitiveBodyFields {
    /// Shared sensitive field set used by `qubit-sanitize`.
    fields: SensitiveFields,
}

impl SensitiveBodyFields {
    /// Creates an empty set.
    ///
    /// # Returns
    /// Empty [`SensitiveBodyFields`].
    pub fn new() -> Self {
        Self {
            fields: SensitiveFields::new(),
        }
    }

    /// Returns whether `name` is sensitive.
    ///
    /// # Parameters
    /// - `name`: Structured body field name.
    ///
    /// # Returns
    /// `true` if the field value should be masked.
    pub fn contains(&self, name: &str) -> bool {
        self.fields.contains(name)
    }

    /// Inserts one field name.
    ///
    /// # Parameters
    /// - `name`: Field name to mark sensitive.
    pub fn insert(&mut self, name: &str) {
        self.fields.insert(name, SensitivityLevel::High);
    }

    /// Inserts many field names.
    ///
    /// # Parameters
    /// - `names`: Field names to mark sensitive.
    pub fn extend<I, S>(&mut self, names: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for name in names {
            self.insert(name.as_ref());
        }
    }

    /// Clears all field names.
    pub fn clear(&mut self) {
        self.fields = SensitiveFields::new();
    }

    /// Returns the number of stored body field names.
    ///
    /// # Returns
    /// Stored body field count.
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Returns whether no body field names are stored.
    ///
    /// # Returns
    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Iterates canonical body field names.
    ///
    /// # Returns
    /// Iterator over stored canonical body field names.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.fields.iter().map(|(field, _)| field)
    }

    /// Extends this set with another body field set, preserving sensitivity levels.
    ///
    /// # Parameters
    /// - `other`: Source body field set.
    pub(crate) fn extend_from(&mut self, other: &Self) {
        for (field, level) in other.fields.iter() {
            self.fields.insert(field, level);
        }
    }

    /// Creates a core field sanitizer from this body field set.
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

impl Default for SensitiveBodyFields {
    /// Creates a set containing common credential and token field names.
    fn default() -> Self {
        Self {
            fields: SensitiveFields::default(),
        }
    }
}
