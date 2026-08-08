// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use bytes::Bytes;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use http::StatusCode;
use http::header::RETRY_AFTER;
use qubit_http::HttpErrorKind;
use qubit_http::HttpResponse;
use url::Url;

use crate::common::SensitiveChoice;

#[tokio::test]
async fn test_http_response_text_decode_error_contains_status_and_url() {
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(&[0xFF, 0xFE]),
        Url::parse("https://example.com/bin").unwrap(),
        Method::GET,
    );
    let error = response.text().await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert_eq!(error.status, Some(StatusCode::OK));
    assert_eq!(
        error.url,
        Some(Url::parse("https://example.com/bin").unwrap())
    );
}

#[test]
fn test_http_response_debug_masks_sensitive_values() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "set-cookie",
        HeaderValue::from_static("session=debug-cookie-secret"),
    );
    let mut url = Url::parse(
        "https://debug-user:debug-url-secret@example.com/data?access_token=debug-query-secret",
    )
    .expect("URL should parse");
    url.set_fragment(Some("debug-fragment-secret"));
    let response = HttpResponse::new(
        StatusCode::OK,
        headers,
        Bytes::from_static(b"debug-response-body-secret"),
        url,
        Method::GET,
    );

    let debug = format!("{response:?}");

    assert!(!debug.contains("debug-user"));
    assert!(!debug.contains("debug-url-secret"));
    assert!(!debug.contains("debug-query-secret"));
    assert!(!debug.contains("debug-fragment-secret"));
    assert!(!debug.contains("debug-cookie-secret"));
    assert!(!debug.contains("debug-response-body-secret"));
    assert!(debug.contains("****"));
}

#[tokio::test]
async fn test_http_response_json_decode_error_contains_status_and_url() {
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"not-json"),
        Url::parse("https://example.com/json").unwrap(),
        Method::GET,
    );
    let error = response.json::<serde_json::Value>().await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert_eq!(error.status, Some(StatusCode::OK));
    assert_eq!(
        error.url,
        Some(Url::parse("https://example.com/json").unwrap())
    );
}

#[test]
fn test_http_response_is_success_reports_status_class() {
    let response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"ok"),
        Url::parse("https://example.com/ok").unwrap(),
        Method::GET,
    );

    assert!(response.is_success());
    assert!(
        !HttpResponse::new(
            StatusCode::BAD_REQUEST,
            HeaderMap::new(),
            Bytes::new(),
            Url::parse("https://example.com/bad").unwrap(),
            Method::GET,
        )
        .is_success()
    );
}

#[test]
fn test_http_response_meta_accessor_returns_shared_metadata() {
    let response = HttpResponse::new(
        StatusCode::ACCEPTED,
        HeaderMap::new(),
        Bytes::from_static(b"queued"),
        Url::parse("https://example.com/jobs/1").unwrap(),
        Method::POST,
    );

    let meta = response.meta();
    assert_eq!(meta.status(), StatusCode::ACCEPTED);
    assert_eq!(
        meta.url(),
        &Url::parse("https://example.com/jobs/1").unwrap()
    );
    assert_eq!(meta.method(), &Method::POST);
}

#[test]
fn test_http_response_retry_after_hint_handles_applicable_status_and_past_date()
{
    let mut headers = HeaderMap::new();
    headers.insert(
        RETRY_AFTER,
        HeaderValue::from_static("Wed, 21 Oct 2015 07:28:00 GMT"),
    );
    let response = HttpResponse::new(
        StatusCode::SERVICE_UNAVAILABLE,
        headers.clone(),
        Bytes::new(),
        Url::parse("https://example.com/retry-after").unwrap(),
        Method::GET,
    );
    assert_eq!(response.retry_after_hint(), Some(std::time::Duration::ZERO));

    let success = HttpResponse::new(
        StatusCode::OK,
        headers,
        Bytes::new(),
        Url::parse("https://example.com/no-retry-after").unwrap(),
        Method::GET,
    );
    assert_eq!(success.retry_after_hint(), None);
}

#[tokio::test]
async fn test_http_response_text_success_returns_body() {
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"hello"),
        Url::parse("https://example.com/utf8").unwrap(),
        Method::GET,
    );

    let text = response.text().await.expect("valid utf8 should decode");
    assert_eq!(text, "hello");
}

#[tokio::test]
async fn test_http_response_json_success_decodes_value() {
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"{\"n\":42}"),
        Url::parse("https://example.com/json-ok").unwrap(),
        Method::GET,
    );

    let value = response
        .json::<serde_json::Value>()
        .await
        .expect("json payload should decode");
    assert_eq!(value["n"], 42);
}

#[tokio::test]
async fn test_http_response_json_rejects_markdown_fence() {
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"```json\n{\"n\":42}\n```"),
        Url::parse("https://example.com/json-fence").unwrap(),
        Method::GET,
    );

    let error = response
        .json::<serde_json::Value>()
        .await
        .expect_err("strict HTTP response JSON must reject Markdown fences");
    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert_eq!(error.status, Some(StatusCode::OK));
}

#[tokio::test]
async fn test_http_response_json_redacts_deserializer_value() {
    const SECRET: &str = "HTTP_TOP_SECRET";
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from(format!("\"{SECRET}\"")),
        Url::parse("https://example.com/secure-json")
            .expect("test URL must parse"),
        Method::GET,
    );
    let error = response
        .json::<SensitiveChoice>()
        .await
        .expect_err("unknown enum variant must fail");
    assert!(!error.message.contains(SECRET));
    assert_eq!(error.status, Some(StatusCode::OK));
    assert_eq!(
        error.url,
        Some(
            Url::parse("https://example.com/secure-json")
                .expect("test URL must parse")
        )
    );
    let source = std::error::Error::source(&error).expect(
        "HTTP JSON decode errors must retain the redacted decoder source",
    );
    let decode_error = source
        .downcast_ref::<qubit_json::JsonDecodeError>()
        .expect("HTTP JSON decode source must be JsonDecodeError");
    assert_eq!(
        decode_error.privacy_policy(),
        qubit_json::ErrorPrivacyPolicy::Redacted,
    );
    assert!(!decode_error.to_string().contains(SECRET));
}
