/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use qubit_http::SensitiveQueryParams;

#[test]
fn test_sensitive_query_params_case_insensitive() {
    let mut params = SensitiveQueryParams::new();

    params.insert(" Api_Key ");

    assert!(params.contains("api_key"));
    assert!(params.contains("API_KEY"));
}

#[test]
fn test_sensitive_query_params_canonicalizes_common_name_styles() {
    let params = SensitiveQueryParams::default();

    assert!(params.contains("accessToken"));
    assert!(params.contains("clientSecret"));
    assert!(params.contains("refresh-token"));
    assert!(params.contains("id_token"));
    assert!(params.contains("apiKey"));
}

#[test]
fn test_sensitive_query_params_contains_uses_suffix_matching() {
    let mut params = SensitiveQueryParams::default();
    params.insert("tenant-marker");

    assert!(params.contains("openai_access_token"));
    assert!(params.contains("tenant_marker"));
    assert!(params.contains("request_tenant_marker"));
}

#[test]
fn test_sensitive_query_params_extend_clear_and_ignore_blank() {
    let mut params = SensitiveQueryParams::new();

    params.extend(["password", " ", "Token"]);
    assert_eq!(params.len(), 2);
    assert!(!params.is_empty());
    assert_eq!(params.iter().collect::<Vec<_>>(), vec!["password", "token"]);
    assert!(params.contains("PASSWORD"));
    assert!(params.contains("token"));

    params.clear();
    assert_eq!(params.len(), 0);
    assert!(params.is_empty());
    assert!(!params.contains("password"));
    assert!(!params.contains(""));
}

#[test]
fn test_sensitive_query_params_default_contains_common_names() {
    let params = SensitiveQueryParams::default();

    assert!(params.contains("access_token"));
    assert!(params.contains("refresh_token"));
    assert!(params.contains("client_secret"));
}
