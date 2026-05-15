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
use qubit_http::HttpRequestBody;

#[test]
fn test_http_request_body_variants_store_expected_payloads() {
    assert_eq!(HttpRequestBody::default(), HttpRequestBody::Empty);
    assert_eq!(HttpRequestBody::Empty.to_string(), "empty");

    let text = HttpRequestBody::Text("hello".to_string());
    let bytes = HttpRequestBody::Bytes(Bytes::from_static(b"hello"));

    assert_eq!(text, HttpRequestBody::Text("hello".to_string()));
    assert_eq!(bytes, HttpRequestBody::Bytes(Bytes::from_static(b"hello")));
}

#[test]
fn test_http_request_body_debug_hides_raw_payloads() {
    let body = HttpRequestBody::Json(Bytes::from_static(
        br#"{"password":"debug-body-secret","user":"alice"}"#,
    ));

    let debug = format!("{body:?}");

    assert!(debug.contains("Json"));
    assert!(debug.contains("len"));
    assert!(!debug.contains("debug-body-secret"));
    assert!(!debug.contains("alice"));
}

#[test]
fn test_http_request_body_debug_covers_all_variants_without_payloads() {
    let bodies = [
        HttpRequestBody::Empty,
        HttpRequestBody::Bytes(Bytes::from_static(b"debug-bytes-secret")),
        HttpRequestBody::Text("debug-text-secret".to_string()),
        HttpRequestBody::Json(Bytes::from_static(br#"{"token":"debug-json-secret"}"#)),
        HttpRequestBody::Form(Bytes::from_static(b"token=debug-form-secret")),
        HttpRequestBody::Multipart(Bytes::from_static(b"debug-multipart-secret")),
        HttpRequestBody::Ndjson(Bytes::from_static(br#"{"token":"debug-ndjson-secret"}"#)),
        HttpRequestBody::Stream(vec![
            Bytes::from_static(b"debug-stream-secret-1"),
            Bytes::from_static(b"debug-stream-secret-2"),
        ]),
    ];

    let debug = bodies
        .iter()
        .map(|body| format!("{body:?}"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(debug.contains("Empty"));
    assert!(debug.contains("Bytes"));
    assert!(debug.contains("Text"));
    assert!(debug.contains("Json"));
    assert!(debug.contains("Form"));
    assert!(debug.contains("Multipart"));
    assert!(debug.contains("Ndjson"));
    assert!(debug.contains("Stream"));
    assert!(debug.contains("chunks"));
    assert!(!debug.contains("debug-bytes-secret"));
    assert!(!debug.contains("debug-text-secret"));
    assert!(!debug.contains("debug-json-secret"));
    assert!(!debug.contains("debug-form-secret"));
    assert!(!debug.contains("debug-multipart-secret"));
    assert!(!debug.contains("debug-ndjson-secret"));
    assert!(!debug.contains("debug-stream-secret"));
}
