/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use qubit_http::SensitiveHttpHeaders;

#[test]
fn test_sensitive_headers_default_is_case_normalized() {
    let headers = SensitiveHttpHeaders::default();
    assert!(!headers.is_empty());
    assert!(headers.contains("AUTHORIZATION"));
    assert!(headers.contains("authorization"));
}

#[test]
fn test_sensitive_headers_case_insensitive() {
    let mut headers = SensitiveHttpHeaders::new();
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
    let mut headers = SensitiveHttpHeaders::new();
    headers.extend(["Authorization", "Cookie", "Set-Cookie"]);
    assert_eq!(headers.len(), 3);
    assert!(!headers.is_empty());
    headers.clear();
    assert!(headers.is_empty());
}

#[test]
fn test_sensitive_headers_iter_returns_normalized_names() {
    let mut headers = SensitiveHttpHeaders::new();
    headers.insert(" Content-Type ");
    headers.insert("X-Custom");

    let names: Vec<_> = headers.iter().collect();
    assert_eq!(names, vec!["content-type", "x-custom"]);
}

#[test]
fn test_sensitive_headers_insert_ignores_empty_or_whitespace_values() {
    let mut headers = SensitiveHttpHeaders::new();
    headers.insert("");
    headers.insert("   ");
    headers.insert("Authorization");

    assert_eq!(headers.len(), 1);
    assert!(headers.contains("authorization"));
}
