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
    /// Creates an empty set without built-in names.
    ///
    /// # Returns
    /// Empty [`SensitiveHttpHeaders`].
    pub fn new() -> Self {
        Self {
            headers: BTreeSet::new(),
        }
    }

    /// Returns whether `header_name` is treated as sensitive.
    ///
    /// # Parameters
    /// - `header_name`: Header name to test.
    ///
    /// # Returns
    /// `true` if values for this header should be masked.
    pub fn contains(&self, header_name: &str) -> bool {
        self.headers.contains(&header_name.to_lowercase())
    }

    /// Inserts one header name after trimming and lowercasing.
    ///
    /// # Parameters
    /// - `header_name`: Header name to mark sensitive.
    pub fn insert(&mut self, header_name: &str) {
        let value = header_name.trim().to_lowercase();
        if !value.is_empty() {
            self.headers.insert(value);
        }
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
        self.headers.clear();
    }

    /// Returns the number of stored header names.
    ///
    /// # Returns
    /// Stored header count.
    pub fn len(&self) -> usize {
        self.headers.len()
    }

    /// Returns whether no header names are stored.
    ///
    /// # Returns
    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }

    /// Iterates normalized header names.
    ///
    /// # Returns
    /// Iterator over lowercase header names.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.headers.iter().map(String::as_str)
    }
}

impl Default for SensitiveHttpHeaders {
    /// Creates a set containing built-in sensitive header names.
    fn default() -> Self {
        let mut result = SensitiveHttpHeaders::new();
        result.extend(DEFAULT_SENSITIVE_HEADER_NAMES);
        result
    }
}
