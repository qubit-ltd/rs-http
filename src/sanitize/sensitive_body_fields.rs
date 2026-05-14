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

/// Case-insensitive set of structured body field names whose values should be masked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensitiveBodyFields {
    /// Normalized lowercase body field names.
    names: BTreeSet<String>,
}

impl SensitiveBodyFields {
    /// Creates an empty set.
    ///
    /// # Returns
    /// Empty [`SensitiveBodyFields`].
    pub fn new() -> Self {
        Self {
            names: BTreeSet::new(),
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
        self.names.contains(&name.to_lowercase())
    }

    /// Inserts one field name.
    ///
    /// # Parameters
    /// - `name`: Field name to mark sensitive.
    pub fn insert(&mut self, name: &str) {
        let value = name.trim().to_lowercase();
        if !value.is_empty() {
            self.names.insert(value);
        }
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
        self.names.clear();
    }

    /// Returns the number of stored body field names.
    ///
    /// # Returns
    /// Stored body field count.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Returns whether no body field names are stored.
    ///
    /// # Returns
    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Iterates normalized body field names.
    ///
    /// # Returns
    /// Iterator over lowercase body field names.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }
}

impl Default for SensitiveBodyFields {
    /// Creates a set containing common credential and token field names.
    fn default() -> Self {
        let mut result = Self::new();
        result.extend([
            "access_token",
            "api_key",
            "authorization",
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
