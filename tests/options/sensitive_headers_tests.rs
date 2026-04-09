/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use qubit_http::SensitiveHeaders;

#[test]
fn test_sensitive_headers_case_insensitive() {
    let mut headers = SensitiveHeaders::new();
    headers.insert("Authorization");
    headers.insert("X-Api-Key");

    assert!(headers.contains("authorization"));
    assert!(headers.contains("AUTHORIZATION"));
    assert!(headers.contains("x-api-key"));
    assert!(headers.contains("X-API-KEY"));
    assert!(!headers.contains("content-type"));
}

#[test]
fn test_sensitive_headers_extend_and_clear() {
    let mut headers = SensitiveHeaders::new();
    headers.extend(["Authorization", "Cookie", "Set-Cookie"]);
    assert_eq!(headers.len(), 3);
    assert!(!headers.is_empty());
    headers.clear();
    assert!(headers.is_empty());
}
