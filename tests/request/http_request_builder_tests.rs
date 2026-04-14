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
use http::{HeaderMap, HeaderValue, Method};
use qubit_http::{HttpErrorKind, HttpRequestBody, HttpRequestBuilder, HttpRetryMethodPolicy};
use serde::ser::{Error as _, Serializer};

struct FailingSerialize;

impl serde::Serialize for FailingSerialize {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Err(S::Error::custom("boom"))
    }
}

#[test]
fn test_request_builder_with_query_params() {
    let request = HttpRequestBuilder::new(Method::GET, "/v1/test")
        .query_param("a", "1")
        .query_param("b", "2")
        .build();

    assert_eq!(request.method, Method::GET);
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
    let result = HttpRequestBuilder::new(Method::GET, "/").header("Invalid Header", "value");
    assert!(result.is_err());
}

#[test]
fn test_request_builder_text_body_sets_content_type() {
    let request = HttpRequestBuilder::new(Method::POST, "/v1/text")
        .text_body("hello world")
        .build();

    assert_eq!(request.method, Method::POST);
    assert_eq!(request.path, "/v1/text");
    assert_eq!(
        request
            .headers
            .get(CONTENT_TYPE)
            .expect("text body should set Content-Type"),
        "text/plain; charset=utf-8"
    );
    match request.body {
        HttpRequestBody::Text(text) => assert_eq!(text, "hello world"),
        _ => panic!("expected text body"),
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
    let request = HttpRequestBuilder::new(Method::POST, "/v1/json")
        .json_body(&payload)
        .expect("valid JSON payload should serialize")
        .build();

    assert_eq!(
        request
            .headers
            .get(CONTENT_TYPE)
            .expect("json body should set Content-Type"),
        "application/json"
    );

    match request.body {
        HttpRequestBody::Json(bytes) => {
            let body: serde_json::Value =
                serde_json::from_slice(&bytes).expect("JSON body bytes should decode");
            assert_eq!(body["name"], "alpha");
            assert_eq!(body["value"], 42);
        }
        _ => panic!("expected JSON body"),
    }
}

#[test]
fn test_request_builder_bytes_body_and_timeout() {
    let request = HttpRequestBuilder::new(Method::PUT, "/v1/blob")
        .bytes_body(Bytes::from_static(b"abc123"))
        .timeout(Duration::from_secs(5))
        .build();

    assert_eq!(request.request_timeout, Some(Duration::from_secs(5)));
    match request.body {
        HttpRequestBody::Bytes(bytes) => assert_eq!(bytes, Bytes::from_static(b"abc123")),
        _ => panic!("expected bytes body"),
    }
}

#[test]
fn test_request_builder_query_params_headers_and_text_body_preserve_existing_content_type() {
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/custom; charset=utf-8"),
    );
    headers.insert(
        http::header::HeaderName::from_static("x-extra"),
        HeaderValue::from_static("present"),
    );

    let request = HttpRequestBuilder::new(Method::POST, "/v1/text")
        .query_params([("a", "1"), ("b", "2")])
        .headers(headers)
        .text_body("hello")
        .build();

    assert_eq!(
        request.query,
        vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ]
    );
    assert_eq!(
        request
            .headers
            .get(CONTENT_TYPE)
            .expect("existing content-type should be kept"),
        "text/custom; charset=utf-8"
    );
    assert_eq!(
        request
            .headers
            .get("x-extra")
            .expect("custom header should be preserved"),
        "present"
    );
    match request.body {
        HttpRequestBody::Text(text) => assert_eq!(text, "hello"),
        _ => panic!("expected text body"),
    }
}

#[test]
fn test_request_builder_json_body_preserves_existing_content_type() {
    let request = HttpRequestBuilder::new(Method::POST, "/v1/json")
        .header(CONTENT_TYPE.as_str(), "application/vnd.test+json")
        .expect("custom content-type header should be valid")
        .json_body(&serde_json::json!({ "ok": true }))
        .expect("valid JSON payload should serialize")
        .build();

    assert_eq!(
        request
            .headers
            .get(CONTENT_TYPE)
            .expect("existing content-type should be kept"),
        "application/vnd.test+json"
    );
    match request.body {
        HttpRequestBody::Json(bytes) => {
            let body: serde_json::Value =
                serde_json::from_slice(&bytes).expect("JSON body bytes should decode");
            assert_eq!(body["ok"], true);
        }
        _ => panic!("expected JSON body"),
    }
}

#[test]
fn test_request_builder_json_body_serialization_failure_returns_decode_error() {
    let error = HttpRequestBuilder::new(Method::POST, "/v1/json")
        .json_body(&FailingSerialize)
        .expect_err("failing serializer should return decode error");

    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert!(error.message.contains("Failed to encode JSON body"));
}

#[test]
fn test_request_builder_retry_override_options() {
    let request = HttpRequestBuilder::new(Method::POST, "/v1/retry")
        .force_retry()
        .retry_method_policy(HttpRetryMethodPolicy::AllMethods)
        .honor_retry_after(true)
        .build();

    assert!(request.retry_override.is_force_enable());
    assert!(!request.retry_override.is_force_disable());
    assert_eq!(
        request.retry_override.method_policy(),
        Some(HttpRetryMethodPolicy::AllMethods)
    );
    assert!(request.retry_override.should_honor_retry_after());
}
