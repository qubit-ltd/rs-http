// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared helpers for reading [`qubit_config::ConfigReader`] keys into option
//! structs.
//!
//! ## Standard configuration schema
//!
//! Keys are **relative** to the [`qubit_config::ConfigReader`] in use (often a
//! [`qubit_config::ConfigSection`] from
//! [`qubit_config::ConfigReader::section`]).
//!
//! ```text
//! base_url                   – Url
//! ipv4_only                  – bool
//!
//! timeouts.*                 – nested timeouts (via `section("timeouts")`)
//! proxy.*                    – nested proxy
//! logging.*                  – nested logging
//! retry.*                    – nested retry
//! sse.json_mode              – String (`LENIENT` / `STRICT`)
//! sse.max_line_bytes         – usize
//! sse.max_frame_bytes        – usize
//! log_redaction.sensitive_headers       – String-compatible values
//! log_redaction.sensitive_query_params  – String-compatible values
//! log_redaction.sensitive_body_fields   – String-compatible values
//! log_redaction.url_path_policy         – `redact` or `preserve`
//! log_redaction.excluded_sensitive_headers      – String-compatible values
//! log_redaction.excluded_sensitive_query_params – String-compatible values
//! log_redaction.excluded_sensitive_body_fields  – String-compatible values
//!
//! default_headers.*          – String-compatible values (sub-key form)
//! ```

use std::collections::HashMap;

use http::HeaderMap;
use http::HeaderName;
use http::HeaderValue;
use qubit_config::ConfigError;
use qubit_config::ConfigReader;
use qubit_config::ConfigResult;

use super::HttpConfigError;

/// Resolves an explicitly reader-relative domain error to its root path.
/// Callers must propagate configuration read errors directly: those paths
/// have already been resolved by qubit-config.
pub(crate) fn resolve_config_error<R>(config: &R, mut error: HttpConfigError) -> HttpConfigError
where
    R: ConfigReader + ?Sized,
{
    error.path = match config.resolve_key(&error.path) {
        Ok(path) => path,
        Err(error) => return HttpConfigError::from(error),
    };
    error
}

/// Rejects keys outside a component's declared fields and open child sections.
/// Returns sorted root-relative unknown paths; unrelated siblings outside the
/// supplied reader are not inspected. Open sections accept descendants only.
pub(crate) fn ensure_known_config_keys<R>(config: &R, fields: &[&str], sections: &[&str]) -> ConfigResult<()>
where
    R: ConfigReader + ?Sized,
{
    let mut paths = Vec::new();
    for (key, _) in config.iter() {
        if fields.contains(&key)
            || sections
                .iter()
                .any(|section| key.strip_prefix(section).is_some_and(|suffix| suffix.starts_with('.')))
        {
            continue;
        }
        paths.push(config.resolve_key(key)?);
    }
    if paths.is_empty() {
        Ok(())
    } else {
        paths.sort();
        paths.dedup();
        Err(ConfigError::UnknownProperties { paths })
    }
}

/// Resolves validation errors whose schema explicitly uses a component prefix.
/// The prefix belongs to the validator, regardless of the reader's scope.
pub(crate) fn resolve_component_error<R>(config: &R, mut error: HttpConfigError, component: &str) -> HttpConfigError
where
    R: ConfigReader + ?Sized,
{
    if let Some(relative) = error
        .path
        .strip_prefix(component)
        .and_then(|suffix| suffix.strip_prefix('.'))
    {
        error.path = relative.to_owned();
    }
    resolve_config_error(config, error)
}

/// Reads an optional fixed-width unsigned value and converts it to `usize`.
///
/// Configuration data stays platform-independent as `u64`; conversion to the
/// platform-sized type happens only at the HTTP API boundary.
pub(crate) fn get_optional_usize<R>(config: &R, key: &str) -> ConfigResult<Option<usize>>
where
    R: ConfigReader + ?Sized,
{
    let Some(value) = config.get_optional::<u64>(key)? else {
        return Ok(None);
    };
    match usize::try_from(value) {
        Ok(value) => Ok(Some(value)),
        Err(_) => Err(ConfigError::DeserializeError {
            path: config.resolve_key(key)?,
            message: "configuration value exceeds the platform usize range".to_string(),
            source: None,
        }),
    }
}

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
pub(crate) fn hashmap_to_headermap(path: &str, map: HashMap<String, String>) -> Result<HeaderMap, HttpConfigError> {
    let mut header_map = HeaderMap::new();
    for (name, value) in map {
        let entry_path = format!("{path}.{name}");
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
            HttpConfigError::invalid_header(entry_path.clone(), format!("Invalid header name '{}': {}", name, e))
        })?;
        let header_value = HeaderValue::from_str(&value).map_err(|e| {
            HttpConfigError::invalid_header(entry_path, format!("Invalid header value for '{}': {}", name, e))
        })?;
        header_map.insert(header_name, header_value);
    }
    Ok(header_map)
}
