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

#[test]
fn test_http_response_text_decode_error_contains_status_and_url() {
    let response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(&[0xFF, 0xFE]),
        Url::parse("https://example.com/bin").unwrap(),
        Method::GET,
    );
    let error = response.text().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert_eq!(error.status, Some(StatusCode::OK));
    assert_eq!(
        error.url,
        Some(Url::parse("https://example.com/bin").unwrap())
    );
}

#[test]
fn test_http_response_json_decode_error_contains_status_and_url() {
    let response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"not-json"),
        Url::parse("https://example.com/json").unwrap(),
        Method::GET,
    );
    let error = response.json::<serde_json::Value>().unwrap_err();
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
fn test_http_response_text_success_returns_body() {
    let response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"hello"),
        Url::parse("https://example.com/utf8").unwrap(),
        Method::GET,
    );

    let text = response.text().expect("valid utf8 should decode");
    assert_eq!(text, "hello");
}

#[test]
fn test_http_response_json_success_decodes_value() {
    let response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"{\"n\":42}"),
        Url::parse("https://example.com/json-ok").unwrap(),
        Method::GET,
    );

    let value = response
        .json::<serde_json::Value>()
        .expect("json payload should decode");
    assert_eq!(value["n"], 42);
}
