/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::Duration;

use bytes::Bytes;
use http::header::CONTENT_TYPE;
use qubit_http::{HttpRequestBody, HttpRequestBuilder};

#[test]
fn test_request_builder_with_query_params() {
    let request = HttpRequestBuilder::new(http::Method::GET, "/v1/test")
        .query_param("a", "1")
        .query_param("b", "2")
        .build();

    assert_eq!(request.method, http::Method::GET);
    assert_eq!(request.path, "/v1/test");
    assert_eq!(
        request.query,
        vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string())
        ]
    );
}

#[test]
fn test_request_builder_header_validation() {
    let result = HttpRequestBuilder::new(http::Method::GET, "/").header("Invalid Header", "value");
    assert!(result.is_err());
}

#[test]
fn test_request_builder_text_body_sets_content_type() {
    let request = HttpRequestBuilder::new(http::Method::POST, "/v1/text")
        .text_body("hello world")
        .build();

    assert_eq!(request.method, http::Method::POST);
    assert_eq!(request.path, "/v1/text");
    assert_eq!(
        request.headers.get(CONTENT_TYPE).unwrap(),
        "text/plain; charset=utf-8"
    );
    match request.body {
        HttpRequestBody::Text(text) => assert_eq!(text, "hello world"),
        _ => panic!("Expected text body"),
    }
}

#[test]
fn test_request_builder_json_body_sets_content_type_and_payload() {
    #[derive(serde::Serialize)]
    struct Payload {
        name: String,
        value: i32,
    }

    let payload = Payload {
        name: "alpha".to_string(),
        value: 42,
    };
    let request = HttpRequestBuilder::new(http::Method::POST, "/v1/json")
        .json_body(&payload)
        .unwrap()
        .build();

    assert_eq!(
        request.headers.get(CONTENT_TYPE).unwrap(),
        "application/json"
    );

    match request.body {
        HttpRequestBody::Json(bytes) => {
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["name"], "alpha");
            assert_eq!(body["value"], 42);
        }
        _ => panic!("Expected JSON body"),
    }
}

#[test]
fn test_request_builder_bytes_body_and_timeout() {
    let request = HttpRequestBuilder::new(http::Method::PUT, "/v1/blob")
        .bytes_body(Bytes::from_static(b"abc123"))
        .timeout(Duration::from_secs(5))
        .build();

    assert_eq!(request.request_timeout, Some(Duration::from_secs(5)));
    match request.body {
        HttpRequestBody::Bytes(bytes) => assert_eq!(bytes, Bytes::from_static(b"abc123")),
        _ => panic!("Expected bytes body"),
    }
}
