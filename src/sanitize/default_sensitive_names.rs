/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Built-in sensitive names used by log sanitization defaults.

/// Built-in header names preloaded into [`crate::SensitiveHttpHeaders::default`].
pub const DEFAULT_SENSITIVE_HEADER_NAMES: [&str; 21] = [
    "Authorization",
    "Proxy-Authorization",
    "Api-Key",
    "X-Api-Key",
    "Bearer",
    "Cookie",
    "Set-Cookie",
    "Secret-Key",
    "Client-Secret",
    "Access-Token",
    "Refresh-Token",
    "Private-Token",
    "Session-Token",
    "JWT-Token",
    "Password",
    "X-Auth-Password",
    "X-Client-ID",
    "X-Client-Secret",
    "X-Auth-Token",
    "X-Auth-App-Token",
    "X-Auth-User-Token",
];

/// Built-in query parameter names preloaded into [`crate::SensitiveQueryParams::default`].
pub const DEFAULT_SENSITIVE_QUERY_PARAM_NAMES: [&str; 8] = [
    "access_token",
    "api_key",
    "client_secret",
    "id_token",
    "password",
    "refresh_token",
    "secret",
    "token",
];

/// Built-in body field names preloaded into [`crate::SensitiveBodyFields::default`].
pub const DEFAULT_SENSITIVE_BODY_FIELD_NAMES: [&str; 9] = [
    "access_token",
    "api_key",
    "authorization",
    "client_secret",
    "id_token",
    "password",
    "refresh_token",
    "secret",
    "token",
];

/// Canonicalizes structured sensitive names for query/body matching.
///
/// # Parameters
/// - `name`: Raw field or parameter name.
///
/// # Returns
/// Lowercase name with common word separators removed.
pub(crate) fn canonicalize_structured_sensitive_name(name: &str) -> String {
    name.trim()
        .chars()
        .filter(|ch| !matches!(ch, '_' | '-'))
        .flat_map(char::to_lowercase)
        .collect()
}
