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
fn test_sensitive_query_params_extend_clear_and_ignore_blank() {
    let mut params = SensitiveQueryParams::new();

    params.extend(["password", " ", "Token"]);
    assert!(params.contains("PASSWORD"));
    assert!(params.contains("token"));

    params.clear();
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
