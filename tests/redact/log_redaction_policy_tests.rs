// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_http::{
    LogRedactionPolicy,
    LogRedactionPolicyBuilder,
};
use qubit_redact::{
    http::{
        BodyBudget,
        DiagnosticBudget,
        UnkeyedJsonValuePolicy,
    },
    PolicyError,
    Sensitivity,
};

/// Verifies that log policy construction consumes an immutable builder.
#[test]
fn test_log_redaction_policy_is_built_immutably() {
    let diagnostic_budget = DiagnosticBudget::new(512, 384)
        .expect("diagnostic budget should be valid");
    let policy = LogRedactionPolicy::builder()
        .raise_header("x-tenant-secret", Sensitivity::Secret)
        .allow_query_exact("public_token")
        .body_budget(BodyBudget::new(128, 256).expect("budget should be valid"))
        .diagnostic_budget(diagnostic_budget)
        .build()
        .expect("policy should be valid");

    assert_eq!(
        policy
            .http_policy()
            .header_policy()
            .sensitivity_for("x-tenant-secret"),
        Some(Sensitivity::Secret),
    );
    assert_eq!(
        policy
            .http_policy()
            .query_policy()
            .sensitivity_for("public_token"),
        None,
    );
    assert_eq!(policy.http_policy().diagnostic_budget(), diagnostic_budget,);
}

/// Verifies that default log policy delegates to the runtime HTTP default.
#[test]
fn test_log_redaction_policy_default_wraps_http_default() {
    assert_eq!(
        LogRedactionPolicy::default().http_policy(),
        &qubit_redact::http::HttpRedactionPolicy::default(),
    );
}

/// Verifies both public default-builder entry points produce the same policy.
#[test]
fn test_log_redaction_policy_default_builder_matches_policy_builder() {
    assert_eq!(
        LogRedactionPolicyBuilder::default().build(),
        LogRedactionPolicy::builder().build(),
    );
}

/// Verifies the builder inherits the runtime's conservative defaults.
#[test]
fn test_log_redaction_policy_builder_inherits_runtime_defaults() {
    let policy = LogRedactionPolicy::builder()
        .build()
        .expect("default policy should be valid");

    assert!(policy
        .http_policy()
        .query_policy()
        .sensitivity_for("access_token")
        .is_some());
}

/// Verifies every field domain retains only its own builder configuration.
#[test]
fn test_log_redaction_policy_builder_keeps_domains_independent() {
    let policy = LogRedactionPolicy::builder()
        .raise_header("x-private-header", Sensitivity::Secret)
        .allow_header_suffix("public_token")
        .raise_query("tenant_query", Sensitivity::High)
        .raise_body("body_custom_alpha", Sensitivity::Secret)
        .raise_body("body_custom_alpha", Sensitivity::Medium)
        .override_body("body_custom_beta", Sensitivity::Low)
        .allow_body_suffix("diagnostic_token")
        .unkeyed_json_value_policy(UnkeyedJsonValuePolicy::PassThrough)
        .build()
        .expect("complete policy should be valid");
    let http = policy.http_policy();

    assert_eq!(
        http.header_policy().sensitivity_for("x-private-header"),
        Some(Sensitivity::Secret),
    );
    assert_eq!(
        http.header_policy().sensitivity_for("service_public_token"),
        None,
    );
    assert_eq!(
        http.query_policy().sensitivity_for("tenant_query"),
        Some(Sensitivity::High),
    );
    assert_eq!(http.header_policy().sensitivity_for("tenant_query"), None,);
    assert_eq!(
        http.body_policy().sensitivity_for("body_custom_alpha"),
        Some(Sensitivity::Secret),
    );
    assert_eq!(
        http.body_policy().sensitivity_for("body_custom_beta"),
        Some(Sensitivity::Low),
    );
    assert_eq!(
        http.body_policy()
            .sensitivity_for("nested_diagnostic_token"),
        None,
    );
    assert_eq!(
        http.query_policy().sensitivity_for("body_custom_alpha"),
        None,
    );
    assert_eq!(
        http.unkeyed_json_value_policy(),
        UnkeyedJsonValuePolicy::PassThrough,
    );
}

/// Verifies invalid header names fail during immutable policy construction.
#[test]
fn test_log_redaction_policy_builder_reports_invalid_header_field() {
    let result = LogRedactionPolicy::builder()
        .raise_header("---", Sensitivity::High)
        .build();

    assert_eq!(result, Err(PolicyError::EmptyFieldName));
}

/// Verifies invalid query names fail during immutable policy construction.
#[test]
fn test_log_redaction_policy_builder_reports_invalid_query_field() {
    let result = LogRedactionPolicy::builder()
        .raise_query("...", Sensitivity::High)
        .build();

    assert_eq!(result, Err(PolicyError::EmptyFieldName));
}

/// Verifies invalid body names fail during immutable policy construction.
#[test]
fn test_log_redaction_policy_builder_reports_invalid_body_field() {
    let result = LogRedactionPolicy::builder()
        .raise_body("___", Sensitivity::High)
        .build();

    assert_eq!(result, Err(PolicyError::EmptyFieldName));
}
