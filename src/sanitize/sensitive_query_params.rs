/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::collections::BTreeSet;

use super::default_sensitive_names::{
    canonicalize_structured_sensitive_name,
    DEFAULT_SENSITIVE_QUERY_PARAM_NAMES,
};

/// Set of query parameter names whose values should be masked.
///
/// Names are matched case-insensitively and common `_` / `-` separators are
/// ignored, so `access_token`, `access-token`, and `accessToken` are equivalent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensitiveQueryParams {
    /// Canonical query parameter names.
    names: BTreeSet<String>,
}

impl SensitiveQueryParams {
    /// Creates an empty set.
    ///
    /// # Returns
    /// Empty [`SensitiveQueryParams`].
    pub fn new() -> Self {
        Self {
            names: BTreeSet::new(),
        }
    }

    /// Returns whether `name` is sensitive.
    ///
    /// # Parameters
    /// - `name`: Query parameter name.
    ///
    /// # Returns
    /// `true` if the value should be masked in logged URLs.
    pub fn contains(&self, name: &str) -> bool {
        self.names
            .contains(&canonicalize_structured_sensitive_name(name))
    }

    /// Inserts one query parameter name.
    ///
    /// # Parameters
    /// - `name`: Query parameter name to mark sensitive.
    pub fn insert(&mut self, name: &str) {
        let value = canonicalize_structured_sensitive_name(name);
        if !value.is_empty() {
            self.names.insert(value);
        }
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
        self.names.clear();
    }

    /// Returns the number of stored query parameter names.
    ///
    /// # Returns
    /// Stored query parameter count.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Returns whether no query parameter names are stored.
    ///
    /// # Returns
    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Iterates canonical query parameter names.
    ///
    /// # Returns
    /// Iterator over stored canonical query parameter names.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }
}

impl Default for SensitiveQueryParams {
    /// Creates a set containing common token-like query parameter names.
    fn default() -> Self {
        let mut result = Self::new();
        result.extend(DEFAULT_SENSITIVE_QUERY_PARAM_NAMES);
        result
    }
}
