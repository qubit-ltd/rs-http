// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_http::{
    LogSanitizePolicy,
    SensitivityLevel,
    TextBodyPolicy,
    UrlPathPolicy,
};

#[test]
fn test_log_sanitize_policy_url_path_policy_defaults_and_round_trips() {
    assert_eq!(
        LogSanitizePolicy::default().url_path_policy(),
        UrlPathPolicy::Preserve,
    );
    assert_eq!(
        LogSanitizePolicy::empty().url_path_policy(),
        UrlPathPolicy::Preserve,
    );

    let mut policy = LogSanitizePolicy::default();
    policy.set_url_path_policy(UrlPathPolicy::Redact);
    assert_eq!(policy.url_path_policy(), UrlPathPolicy::Redact);
    assert_eq!(
        LogSanitizePolicy::default()
            .with_url_path_policy(UrlPathPolicy::Redact)
            .url_path_policy(),
        UrlPathPolicy::Redact,
    );
}

#[test]
fn test_log_sanitize_policy_text_body_policy_defaults_and_round_trips() {
    assert_eq!(
        LogSanitizePolicy::default().text_body_policy(),
        TextBodyPolicy::Redact,
    );
    assert_eq!(
        LogSanitizePolicy::empty().text_body_policy(),
        TextBodyPolicy::Redact,
    );

    let mut policy = LogSanitizePolicy::default();
    policy.set_text_body_policy(TextBodyPolicy::PassThrough);
    assert_eq!(policy.text_body_policy(), TextBodyPolicy::PassThrough);
    assert_eq!(
        LogSanitizePolicy::default()
            .with_text_body_policy(TextBodyPolicy::PassThrough)
            .text_body_policy(),
        TextBodyPolicy::PassThrough,
    );
}

#[test]
fn test_log_sanitize_policy_default_contains_common_sensitive_names() {
    let policy = LogSanitizePolicy::default();

    assert_eq!(
        policy.sensitivity_for_header("Authorization"),
        Some(SensitivityLevel::High),
    );
    assert_eq!(
        policy.sensitivity_for_query_param("access_token"),
        Some(SensitivityLevel::High),
    );
    assert_eq!(
        policy.sensitivity_for_body_field("password"),
        Some(SensitivityLevel::Secret),
    );
}

#[test]
fn test_log_sanitize_policy_clone_and_equality() {
    let mut policy = LogSanitizePolicy::empty();
    policy.insert_sensitive_header("X-Secret", SensitivityLevel::High);
    policy.insert_sensitive_query_param("api_key", SensitivityLevel::High);
    policy.insert_sensitive_body_field("token", SensitivityLevel::High);

    assert_eq!(policy, policy.clone());
    assert!(format!("{policy:?}").contains("LogSanitizePolicy"));
}

#[test]
fn test_log_sanitize_policy_add_is_strongest_and_set_is_explicit() {
    let mut policy = LogSanitizePolicy::default();

    policy.insert_sensitive_body_field("password", SensitivityLevel::Low);
    assert_eq!(
        policy.sensitivity_for_body_field("password"),
        Some(SensitivityLevel::Secret),
    );

    policy.set_sensitive_body_field_level("password", SensitivityLevel::Low);
    assert_eq!(
        policy.sensitivity_for_body_field("password"),
        Some(SensitivityLevel::Low),
    );
}

#[test]
fn test_log_sanitize_policy_domain_facades_extend_set_and_remove() {
    let mut policy = LogSanitizePolicy::empty();

    policy.extend_sensitive_headers(
        ["x-first", "x-second"],
        SensitivityLevel::High,
    );
    policy.set_sensitive_header_level("x-first", SensitivityLevel::Low);
    assert_eq!(
        policy.remove_sensitive_header("x-first"),
        Some(SensitivityLevel::Low),
    );
    assert_eq!(policy.sensitivity_for_header("x-first"), None);

    policy.extend_sensitive_query_params(
        ["first_token", "second_token"],
        SensitivityLevel::High,
    );
    policy
        .set_sensitive_query_param_level("first_token", SensitivityLevel::Low);
    assert_eq!(
        policy.remove_sensitive_query_param("first_token"),
        Some(SensitivityLevel::Low),
    );
    assert_eq!(policy.sensitivity_for_query_param("first_token"), None);

    policy.extend_sensitive_body_fields(
        ["first_secret", "second_secret"],
        SensitivityLevel::Secret,
    );
    policy
        .set_sensitive_body_field_level("first_secret", SensitivityLevel::Low);
    assert_eq!(
        policy.remove_sensitive_body_field("first_secret"),
        Some(SensitivityLevel::Low),
    );
    assert_eq!(policy.sensitivity_for_body_field("first_secret"), None);
}
