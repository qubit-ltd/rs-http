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

use crate::constants::DEFAULT_SENSITIVE_HEADER_NAMES;

/// Case-insensitive set of HTTP header names whose values should be masked in logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensitiveHttpHeaders {
    /// Normalized lowercase header names.
    headers: BTreeSet<String>,
}

impl SensitiveHttpHeaders {
    /// Creates an empty set (no names marked sensitive).
    ///
    /// # Returns
    /// New [`SensitiveHttpHeaders`] without default names; prefer
    /// [`SensitiveHttpHeaders::default`] for built-ins.
    pub fn new() -> Self {
        Self {
            headers: BTreeSet::new(),
        }
    }

    /// Returns whether `header_name` is treated as sensitive (compared case-insensitively).
    ///
    /// # Parameters
    /// - `header_name`: Header name to test (any casing).
    ///
    /// # Returns
    /// `true` if masked in logging helpers.
    pub fn contains(&self, header_name: &str) -> bool {
        self.headers.contains(&header_name.to_lowercase())
    }

    /// Inserts one header name after trimming and lowercasing; ignores empty strings.
    ///
    /// # Parameters
    /// - `header_name`: Name to mark sensitive.
    pub fn insert(&mut self, header_name: &str) {
        let value = header_name.trim().to_lowercase();
        if !value.is_empty() {
            self.headers.insert(value);
        }
    }

    /// Inserts each header from the iterator via [`SensitiveHttpHeaders::insert`].
    ///
    /// # Parameters
    /// - `headers`: Iterator of header name-like values.
    pub fn extend<I, S>(&mut self, headers: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for header in headers {
            self.insert(header.as_ref());
        }
    }

    /// Clears all stored sensitive header names.
    pub fn clear(&mut self) {
        self.headers.clear();
    }

    /// Returns how many sensitive header names are stored.
    ///
    /// # Returns
    /// Count of entries in the internal set.
    pub fn len(&self) -> usize {
        self.headers.len()
    }

    /// Returns whether no sensitive names are registered.
    ///
    /// # Returns
    /// `true` if [`SensitiveHttpHeaders::len`] is zero.
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }

    /// Iterates normalized (lowercase) sensitive header names.
    ///
    /// # Returns
    /// Iterator over string slices owned by `self`.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.headers.iter().map(String::as_str)
    }
}

impl Default for SensitiveHttpHeaders {
    /// Starts with [`crate::DEFAULT_SENSITIVE_HEADER_NAMES`] pre-registered.
    ///
    /// # Returns
    /// Non-empty [`SensitiveHttpHeaders`].
    fn default() -> Self {
        let mut result = SensitiveHttpHeaders::new();
        result.extend(DEFAULT_SENSITIVE_HEADER_NAMES);
        result
    }
}
