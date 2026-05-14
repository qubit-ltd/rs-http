/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use qubit_http::SensitiveBodyFields;

#[test]
fn test_sensitive_body_fields_case_insensitive() {
    let mut fields = SensitiveBodyFields::new();

    fields.insert(" Api_Key ");

    assert!(fields.contains("api_key"));
    assert!(fields.contains("API_KEY"));
}

#[test]
fn test_sensitive_body_fields_extend_clear_and_ignore_blank() {
    let mut fields = SensitiveBodyFields::new();

    fields.extend(["password", " ", "Token"]);
    assert_eq!(fields.len(), 2);
    assert!(!fields.is_empty());
    assert_eq!(fields.iter().collect::<Vec<_>>(), vec!["password", "token"]);
    assert!(fields.contains("PASSWORD"));
    assert!(fields.contains("token"));

    fields.clear();
    assert_eq!(fields.len(), 0);
    assert!(fields.is_empty());
    assert!(!fields.contains("password"));
    assert!(!fields.contains(""));
}

#[test]
fn test_sensitive_body_fields_default_contains_common_names() {
    let fields = SensitiveBodyFields::default();

    assert!(fields.contains("authorization"));
    assert!(fields.contains("refresh_token"));
    assert!(fields.contains("client_secret"));
}
