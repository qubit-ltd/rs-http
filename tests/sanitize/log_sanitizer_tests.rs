/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use bytes::Bytes;
use http::header::{
    HeaderName,
    HeaderValue,
    AUTHORIZATION,
};
use qubit_http::{
    BodyLogContext,
    BodyPreview,
    LogSanitizePolicy,
    LogSanitizer,
};
use url::Url;

#[test]
fn test_log_sanitizer_sanitize_url_masks_sensitive_query_params() {
    let sanitizer = LogSanitizer::default();
    let url = Url::parse("https://example.com/search?q=rust&access_token=secret-token")
        .expect("test URL should parse");

    let sanitized = sanitizer.sanitize_url(&url);

    assert_eq!(
        sanitized,
        "https://example.com/search?q=rust&access_token=****"
    );
}

#[test]
fn test_log_sanitizer_sanitize_header_masks_configured_header_names() {
    let sanitizer = LogSanitizer::default();

    let sanitized = sanitizer.sanitize_header_value(
        &AUTHORIZATION,
        &HeaderValue::from_static("Bearer very-secret-token"),
    );

    assert_eq!(sanitized, "Be****en");
}

#[test]
fn test_log_sanitizer_sanitize_header_keeps_non_sensitive_header_values() {
    let sanitizer = LogSanitizer::default();
    let header_name = HeaderName::from_static("content-type");

    let sanitized = sanitizer
        .sanitize_header_value(&header_name, &HeaderValue::from_static("application/json"));

    assert_eq!(sanitized, "application/json");
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_json_fields() {
    let sanitizer = LogSanitizer::default();
    let body =
        Bytes::from_static(br#"{"user":"alice","password":"secret","nested":{"token":"abc"}}"#);
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("application/json");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(
        sanitized,
        r#"{"nested":{"token":"****"},"password":"****","user":"alice"}"#
    );
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_does_not_leak_truncated_json() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(br#"{"password":"secret","user":"alice","tail":"long"}"#);
    let preview =
        BodyPreview::new(&body, 20, BodyLogContext::Request).with_content_type("application/json");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.starts_with("<redacted: invalid or truncated JSON>"));
    assert!(!sanitized.contains("secret"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_ndjson_fields() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        br#"{"token":"abc","id":1}
{"id":2}"#,
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("application/x-ndjson");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(sanitized, "{\"id\":1,\"token\":\"****\"}\n{\"id\":2}");
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_form_fields() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(b"username=alice&password=secret&city=Shanghai");
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("application/x-www-form-urlencoded");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(sanitized, "username=alice&password=****&city=Shanghai");
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_uses_custom_policy() {
    let mut policy = LogSanitizePolicy::default();
    policy.sensitive_body_fields.clear();
    policy.sensitive_body_fields.insert("customer_id");
    let sanitizer = LogSanitizer::new(policy);
    let body = Bytes::from_static(br#"{"customer_id":"C-001","password":"kept"}"#);
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("application/json");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(sanitized, r#"{"customer_id":"****","password":"kept"}"#);
}
