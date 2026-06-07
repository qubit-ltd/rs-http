// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_http::LogSanitizePolicy;
use qubit_sanitize::{
    SensitiveFields,
    SensitivityLevel,
};

#[test]
fn test_log_sanitize_policy_default_contains_common_sensitive_names() {
    let policy = LogSanitizePolicy::default();

    assert!(policy.sensitive_headers.contains("Authorization"));
    assert!(policy.sensitive_query_params.contains("access_token"));
    assert!(policy.sensitive_body_fields.contains("password"));
}

#[test]
fn test_log_sanitize_policy_clone_and_equality() {
    let mut policy = LogSanitizePolicy {
        sensitive_headers: SensitiveFields::new(),
        sensitive_query_params: SensitiveFields::new(),
        sensitive_body_fields: SensitiveFields::new(),
    };
    policy
        .sensitive_headers
        .insert("X-Secret", SensitivityLevel::High);
    policy
        .sensitive_query_params
        .insert("api_key", SensitivityLevel::High);
    policy
        .sensitive_body_fields
        .insert("token", SensitivityLevel::High);

    assert_eq!(policy, policy.clone());
    assert!(format!("{policy:?}").contains("LogSanitizePolicy"));
}
