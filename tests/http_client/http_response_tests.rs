/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use bytes::Bytes;
use http::HeaderMap;
use http::StatusCode;
use qubit_http::{HttpErrorKind, HttpResponse};
use url::Url;

#[test]
fn test_http_response_text_decode_error_contains_status_and_url() {
    let response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(&[0xFF, 0xFE]),
        Url::parse("https://example.com/bin").unwrap(),
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
    );
    let error = response.json::<serde_json::Value>().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert_eq!(error.status, Some(StatusCode::OK));
    assert_eq!(
        error.url,
        Some(Url::parse("https://example.com/json").unwrap())
    );
}
