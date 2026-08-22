// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use http::StatusCode;
use http::header::RETRY_AFTER;
use http::header::SET_COOKIE;
use qubit_http::HttpResponseMeta;
use qubit_redact::RedactionPolicy;
use qubit_redact::formats::http::UrlPathPolicy;
use url::Url;

#[test]
fn test_http_response_meta_retry_after_only_applies_to_retryable_statuses() {
    let mut headers = HeaderMap::new();
    headers.insert(RETRY_AFTER, HeaderValue::from_static("7"));
    let url = Url::parse("https://example.com/retry").expect("valid URL");
    let rate_limited = HttpResponseMeta::new(
        StatusCode::TOO_MANY_REQUESTS,
        headers.clone(),
        url.clone(),
        Method::GET,
    );
    let success =
        HttpResponseMeta::new(StatusCode::OK, headers, url, Method::GET);

    assert_eq!(
        rate_limited.retry_after_hint(),
        Some(std::time::Duration::from_secs(7))
    );
    assert_eq!(success.retry_after_hint(), None);
}

#[test]
fn test_http_response_meta_debug_masks_sensitive_values() {
    let mut headers = HeaderMap::new();
    headers.insert(
        SET_COOKIE,
        HeaderValue::from_static("session=debug-cookie-secret"),
    );
    let meta = HttpResponseMeta::new(
        StatusCode::OK,
        headers,
        Url::parse("https://debug-user:debug-url-secret@example.test/retry?password=debug-password-secret&access_token=debug-token-secret")
            .expect("valid URL"),
        Method::GET,
    );

    let debug = format!("{meta:?}");

    assert!(!debug.contains("debug-user"));
    assert!(!debug.contains("debug-url-secret"));
    assert!(!debug.contains("debug-password-secret"));
    assert!(!debug.contains("debug-token-secret"));
    assert!(!debug.contains("debug-cookie-secret"));
    assert!(debug.contains("%3Credacted%3E"));
    assert!(debug.contains("****"));
}

#[test]
fn test_http_response_meta_debug_honors_url_path_redaction_policy() {
    let policy_builder = RedactionPolicy::default()
        .to_builder()
        .http(|http| {
            http.url_path(UrlPathPolicy::Redact);
        })
        .expect("test policy should be valid");
    let meta = HttpResponseMeta::new(
        StatusCode::OK,
        HeaderMap::new(),
        Url::parse(
            "https://alice:response-password-secret@example.test/tenant/response-path-secret?access_token=response-query-secret#response-fragment-secret",
        )
        .expect("valid URL"),
        Method::GET,
    )
    .with_log_redaction_policy(
        policy_builder
            .build()
            .expect("log redaction policy should be valid"),
    );

    let debug = format!("{meta:?}");

    assert!(!debug.contains("tenant/response-path-secret"));
    assert!(!debug.contains("alice"));
    assert!(!debug.contains("response-password-secret"));
    assert!(!debug.contains("response-query-secret"));
    assert!(!debug.contains("response-fragment-secret"));
    assert!(debug.contains("/<redacted>?"));
}
