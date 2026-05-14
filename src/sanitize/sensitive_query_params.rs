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

/// Case-insensitive set of query parameter names whose values should be masked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensitiveQueryParams {
    /// Normalized lowercase query parameter names.
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
        self.names.contains(&name.to_lowercase())
    }

    /// Inserts one query parameter name.
    ///
    /// # Parameters
    /// - `name`: Query parameter name to mark sensitive.
    pub fn insert(&mut self, name: &str) {
        let value = name.trim().to_lowercase();
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

    /// Iterates normalized query parameter names.
    ///
    /// # Returns
    /// Iterator over lowercase query parameter names.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }
}

impl Default for SensitiveQueryParams {
    /// Creates a set containing common token-like query parameter names.
    fn default() -> Self {
        let mut result = Self::new();
        result.extend([
            "access_token",
            "api_key",
            "client_secret",
            "id_token",
            "password",
            "refresh_token",
            "secret",
            "token",
        ]);
        result
    }
}
