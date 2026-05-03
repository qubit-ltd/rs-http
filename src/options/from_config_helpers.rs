/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Shared helpers for reading [`qubit_config::ConfigReader`] keys into option structs.
//!
//! ## Standard configuration schema
//!
//! Keys are **relative** to the [`qubit_config::ConfigReader`] in use (often a
//! [`qubit_config::ConfigPrefixView`] from [`qubit_config::ConfigReader::prefix_view`]).
//!
//! ```text
//! base_url                   – Url
//! ipv4_only                  – bool
//!
//! timeouts.*                 – nested timeouts (via `prefix_view("timeouts")`)
//! proxy.*                    – nested proxy
//! logging.*                  – nested logging
//! retry.*                    – nested retry
//! sse.json_mode              – String (`LENIENT` / `STRICT`)
//! sse.max_line_bytes         – usize
//! sse.max_frame_bytes        – usize
//!
//! default_headers.*          – String-compatible values (sub-key form)
//! sensitive_headers          – String-compatible values
//! ```
//!

use std::collections::HashMap;

use http::{
    HeaderMap,
    HeaderName,
    HeaderValue,
};

use super::HttpConfigError;

/// Converts a map of header names to values into an [`HeaderMap`].
///
/// # Parameters
/// - `path`: Configuration path of the header map root.
/// - `map`: Header names and values read from configuration.
///
/// # Returns
/// Parsed [`HeaderMap`].
///
/// # Errors
/// Returns [`HttpConfigError`] with the concrete header entry path when a
/// header name or value is invalid.
pub(crate) fn hashmap_to_headermap(
    path: &str,
    map: HashMap<String, String>,
) -> Result<HeaderMap, HttpConfigError> {
    let mut header_map = HeaderMap::new();
    for (name, value) in map {
        let entry_path = format!("{path}.{name}");
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
            HttpConfigError::invalid_header(
                entry_path.clone(),
                format!("Invalid header name '{}': {}", name, e),
            )
        })?;
        let header_value = HeaderValue::from_str(&value).map_err(|e| {
            HttpConfigError::invalid_header(
                entry_path,
                format!("Invalid header value for '{}': {}", name, e),
            )
        })?;
        header_map.insert(header_name, header_value);
    }
    Ok(header_map)
}
