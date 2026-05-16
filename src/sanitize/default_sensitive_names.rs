/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Default sensitive names used by HTTP log sanitization.

use qubit_sanitize::{
    SensitiveFields,
    SensitivityLevel,
};

/// Default sensitive HTTP header names.
///
/// Names are canonicalized by `qubit-sanitize` before matching, so snake case,
/// kebab case, dotted names, spaced names, and camel case are treated
/// equivalently.
pub const DEFAULT_SENSITIVE_HEADER_NAMES: &[&str] = DEFAULT_SENSITIVE_LOG_FIELD_NAMES;

/// Default sensitive URL query parameter names.
///
/// These names are added to [`crate::SensitiveQueryParams::default`].
pub const DEFAULT_SENSITIVE_QUERY_PARAM_NAMES: &[&str] = DEFAULT_SENSITIVE_LOG_FIELD_NAMES;

/// Default sensitive structured body field names.
///
/// These names are added to [`crate::SensitiveBodyFields::default`].
pub const DEFAULT_SENSITIVE_BODY_FIELD_NAMES: &[&str] = DEFAULT_SENSITIVE_LOG_FIELD_NAMES;

/// Shared conservative default field names for HTTP logs and diagnostics.
const DEFAULT_SENSITIVE_LOG_FIELD_NAMES: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "client_secret",
    "private_key",
    "api_key",
    "x_api_key",
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "jwt",
    "jwt_token",
    "auth_token",
    "authorization",
    "proxy_authorization",
    "cookie",
    "set_cookie",
    "session",
    "session_id",
    "session_token",
    "auth_app_token",
    "auth_user_token",
    "license_key",
];

/// Creates a [`SensitiveFields`] value from one default-name slice.
///
/// # Parameters
/// - `names`: Default sensitive names.
///
/// # Returns
/// Sensitive fields with the crate's default sensitivity levels.
pub(crate) fn default_sensitive_fields(names: &[&str]) -> SensitiveFields {
    let mut fields = SensitiveFields::new();
    for name in names {
        fields.insert(name, default_sensitivity_level(name));
    }
    fields
}

/// Returns the default sensitivity level for one default sensitive name.
///
/// # Parameters
/// - `name`: Default sensitive name.
///
/// # Returns
/// Sensitivity level used by log sanitization.
fn default_sensitivity_level(name: &str) -> SensitivityLevel {
    match name {
        "password" | "passwd" | "secret" | "client_secret" | "private_key" => {
            SensitivityLevel::Secret
        }
        "session" | "session_id" | "license_key" => SensitivityLevel::Medium,
        _ => SensitivityLevel::High,
    }
}
