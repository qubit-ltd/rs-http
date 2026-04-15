/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use bytes::Bytes;
use http::{HeaderMap, Method, StatusCode};
use qubit_http::{HttpErrorKind, HttpResponse};
use url::Url;

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
    assert!(!HttpResponse::new(
        StatusCode::BAD_REQUEST,
        HeaderMap::new(),
        Bytes::new(),
        Url::parse("https://example.com/bad").unwrap(),
        Method::GET,
    )
    .is_success());
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
    assert_eq!(meta.status, StatusCode::ACCEPTED);
    assert_eq!(meta.url, Url::parse("https://example.com/jobs/1").unwrap());
    assert_eq!(meta.method, Method::POST);
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
