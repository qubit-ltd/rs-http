/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
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
//! default_headers.*          – String (sub-key form)
//! sensitive_headers          – Vec<String>
//! ```
//!
//! # Author
//!
//! Haixing Hu

use std::collections::HashMap;

use http::{HeaderMap, HeaderName, HeaderValue};

use super::HttpConfigError;

/// Converts a map of header names to values into an [`HeaderMap`].
pub(crate) fn hashmap_to_headermap(
    path: &str,
    map: HashMap<String, String>,
) -> Result<HeaderMap, HttpConfigError> {
    let mut header_map = HeaderMap::new();
    for (name, value) in map {
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
            HttpConfigError::invalid_header(path, format!("Invalid header name '{}': {}", name, e))
        })?;
        let header_value = HeaderValue::from_str(&value).map_err(|e| {
            HttpConfigError::invalid_header(
                path,
                format!("Invalid header value for '{}': {}", name, e),
            )
        })?;
        header_map.insert(header_name, header_value);
    }
    Ok(header_map)
}
