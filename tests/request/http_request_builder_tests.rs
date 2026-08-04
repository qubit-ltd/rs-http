// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    error::Error as _,
    time::Duration,
};

use bytes::Bytes;
use http::header::CONTENT_TYPE;
use http::{
    HeaderMap,
    HeaderValue,
    Method,
};
use qubit_http::{
    CancellationToken,
    HttpClientFactory,
    HttpClientOptions,
    HttpErrorKind,
    HttpRequestBody,
    HttpRetryMethodPolicy,
};
use qubit_redact::{
    http::HttpFieldContext,
    RedactionPolicy,
    Sensitivity,
};
use serde::ser::{
    Error as _,
    Serializer,
};

struct FailingSerialize;

impl serde::Serialize for FailingSerialize {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Err(S::Error::custom("boom"))
    }
}

fn new_builder(method: Method, path: &str) -> qubit_http::HttpRequestBuilder {
    let client = HttpClientFactory::new()
        .create_default()
        .expect("default options should create client");
    client.request(method, path)
}

#[test]
fn test_request_builder_debug_masks_sensitive_values() {
    let mut options = HttpClientOptions::new();
    options
        .set_base_url("https://api.example.com/root/")
        .expect("base URL should be valid");
    options.log_redaction_policy = RedactionPolicy::default()
        .to_builder()
        .http_raise(
            HttpFieldContext::Header,
            "x-debug-secret",
            Sensitivity::High,
        )
        .expect("the test policy input should be valid")
        .http_raise(HttpFieldContext::Query, "debugToken", Sensitivity::High)
        .expect("the test policy input should be valid")
        .build()
        .expect("log redaction policy should be valid");
    let mut default_headers = HeaderMap::new();
    default_headers.insert(
        "x-debug-secret",
        HeaderValue::from_static("debug-default-header-secret"),
    );
    options.default_headers = default_headers;
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let relative = client
        .request(Method::POST, "items?debugToken=debug-query-secret")
        .query_param("clientSecret", "debug-added-query-secret")
        .header("x-debug-secret", "debug-request-header-secret")
        .expect("header should be accepted")
        .text_body("debug-text-secret")
        .request_timeout(Duration::from_secs(1))
        .expect("positive timeout should be accepted")
        .cancellation_token(CancellationToken::new());
    let absolute = client.request(
        Method::GET,
        "https://debug-user:debug-url-secret@example.com/path",
    );

    let debug = format!("{relative:?}\n{absolute:?}");

    assert!(debug.contains("HttpRequestBuilder"));
    assert!(debug.contains("****"));
    assert!(!debug.contains("debug-default-header-secret"));
    assert!(!debug.contains("debug-request-header-secret"));
    assert!(!debug.contains("debug-query-secret"));
    assert!(!debug.contains("debug-added-query-secret"));
    assert!(!debug.contains("debug-text-secret"));
    assert!(!debug.contains("debug-user"));
    assert!(!debug.contains("debug-url-secret"));
}

#[test]
fn test_request_builder_copies_base_url_and_ipv4_only_defaults() {
    let mut options = HttpClientOptions::default();
    options.set_base_url("https://api.example.com/v1/").unwrap();
    options.ipv4_only = true;

    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let request = client.request(Method::GET, "users").build();
    assert_eq!(
        request.base_url().map(url::Url::as_str),
        Some("https://api.example.com/v1/")
    );
    assert!(request.ipv4_only());
}

#[test]
fn test_request_builder_copies_write_timeout_default_from_client_options() {
    let mut options = HttpClientOptions::default();
    options.timeouts.write_timeout = Duration::from_millis(321);

    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let request = client
        .request(Method::GET, "/v1/default-write-timeout")
        .build();

    assert_eq!(request.write_timeout(), Duration::from_millis(321));
}

#[test]
fn test_request_builder_write_timeout_overrides_default_from_options() {
    let mut options = HttpClientOptions::default();
    options.timeouts.write_timeout = Duration::from_secs(2);

    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let request = client
        .request(Method::GET, "/v1/override-write-timeout")
        .write_timeout(Duration::from_millis(88))
        .expect("positive write timeout should be accepted")
        .build();

    assert_eq!(request.write_timeout(), Duration::from_millis(88));
}

#[test]
fn test_request_builder_write_timeout_rejects_zero() {
    let error = new_builder(Method::GET, "/v1/zero-write-timeout")
        .write_timeout(Duration::ZERO)
        .expect_err("zero write timeout should be rejected");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("write_timeout"));
    assert!(error.message.contains("greater than zero"));
}

#[test]
fn test_request_builder_copies_read_timeout_default_from_client_options() {
    let mut options = HttpClientOptions::default();
    options.timeouts.read_timeout = Duration::from_millis(432);

    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let request = client
        .request(Method::GET, "/v1/default-read-timeout")
        .build();

    assert_eq!(request.read_timeout(), Duration::from_millis(432));
}

#[test]
fn test_request_builder_read_timeout_overrides_default_from_options() {
    let mut options = HttpClientOptions::default();
    options.timeouts.read_timeout = Duration::from_secs(2);

    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let request = client
        .request(Method::GET, "/v1/override-read-timeout")
        .read_timeout(Duration::from_millis(77))
        .expect("positive read timeout should be accepted")
        .build();

    assert_eq!(request.read_timeout(), Duration::from_millis(77));
}

#[test]
fn test_request_builder_read_timeout_rejects_zero() {
    let error = new_builder(Method::GET, "/v1/zero-read-timeout")
        .read_timeout(Duration::ZERO)
        .expect_err("zero read timeout should be rejected");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("read_timeout"));
    assert!(error.message.contains("greater than zero"));
}

#[test]
fn test_request_builder_base_url_method_overrides_default_from_options() {
    let mut options = HttpClientOptions::default();
    options.set_base_url("https://api.example.com/v1/").unwrap();

    let override_base =
        url::Url::parse("https://override.example.com/root/").unwrap();
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let request = client
        .request(Method::GET, "users")
        .base_url(override_base.clone())
        .build();

    assert_eq!(request.base_url(), Some(&override_base));
}

#[test]
fn test_request_builder_ipv4_only_method_overrides_default_from_options() {
    let mut options = HttpClientOptions::default();
    options.ipv4_only = true;

    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let request = client
        .request(Method::GET, "users")
        .ipv4_only(false)
        .build();

    assert!(!request.ipv4_only());
}

#[test]
fn test_request_builder_clear_base_url_method_overrides_default_from_options() {
    let mut options = HttpClientOptions::default();
    options.set_base_url("https://api.example.com/v1/").unwrap();

    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let request = client
        .request(Method::GET, "users")
        .clear_base_url()
        .build();

    assert_eq!(request.base_url(), None);
}

#[test]
fn test_request_builder_with_query_params() {
    let request = new_builder(Method::GET, "/v1/test")
        .query_param("a", "1")
        .query_param("b", "2")
        .build();

    assert_eq!(request.method(), &Method::GET);
    assert_eq!(request.path(), "/v1/test");
    assert_eq!(
        request.query(),
        vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string())
        ]
    );
}

#[test]
fn test_request_builder_header_validation() {
    let result =
        new_builder(Method::GET, "/").header("Invalid Header", "value");
    assert!(result.is_err());
}

#[test]
fn test_request_builder_text_body_sets_content_type() {
    let request = new_builder(Method::POST, "/v1/text")
        .text_body("hello world")
        .build();

    assert_eq!(request.method(), &Method::POST);
    assert_eq!(request.path(), "/v1/text");
    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("text body should set Content-Type"),
        "text/plain; charset=utf-8"
    );
    match request.body() {
        HttpRequestBody::Text(text) => assert_eq!(text.as_str(), "hello world"),
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
    let request = new_builder(Method::POST, "/v1/json")
        .json_body(&payload)
        .expect("valid JSON payload should serialize")
        .build();

    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("json body should set Content-Type"),
        "application/json"
    );

    match request.body() {
        HttpRequestBody::Json(bytes) => {
            let body: serde_json::Value = serde_json::from_slice(bytes)
                .expect("JSON body bytes should decode");
            assert_eq!(body["name"], "alpha");
            assert_eq!(body["value"], 42);
        }
        _ => panic!("expected JSON body"),
    }
}

#[test]
fn test_request_builder_bytes_body_and_request_timeout() {
    let request = new_builder(Method::PUT, "/v1/blob")
        .bytes_body(Bytes::from_static(b"abc123"))
        .request_timeout(Duration::from_secs(5))
        .expect("positive request timeout should be accepted")
        .build();

    assert_eq!(request.request_timeout(), Some(Duration::from_secs(5)));
    match request.body() {
        HttpRequestBody::Bytes(bytes) => {
            assert_eq!(bytes.as_ref(), Bytes::from_static(b"abc123").as_ref())
        }
        _ => panic!("expected bytes body"),
    }
}

#[test]
fn test_request_builder_request_timeout_rejects_zero() {
    let error = new_builder(Method::GET, "/v1/zero-request-timeout")
        .request_timeout(Duration::ZERO)
        .expect_err("zero request timeout should be rejected");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("request_timeout"));
    assert!(error.message.contains("greater than zero"));
}

#[test]
fn test_request_builder_stream_body_preserves_chunk_order() {
    let request = new_builder(Method::PUT, "/v1/stream")
        .stream_body([
            Bytes::from_static(b"alpha"),
            Bytes::from_static(b"-"),
            Bytes::from_static(b"beta"),
        ])
        .build();

    match request.body() {
        HttpRequestBody::Stream(chunks) => {
            assert_eq!(chunks.len(), 3);
            assert_eq!(chunks[0], Bytes::from_static(b"alpha"));
            assert_eq!(chunks[1], Bytes::from_static(b"-"));
            assert_eq!(chunks[2], Bytes::from_static(b"beta"));
        }
        _ => panic!("expected stream body"),
    }
}

#[test]
fn test_request_builder_query_params_headers_and_text_body_preserve_existing_content_type(
) {
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/custom; charset=utf-8"),
    );
    headers.insert(
        http::header::HeaderName::from_static("x-extra"),
        HeaderValue::from_static("present"),
    );

    let request = new_builder(Method::POST, "/v1/text")
        .query_params([("a", "1"), ("b", "2")])
        .headers(headers)
        .text_body("hello")
        .build();

    assert_eq!(
        request.query(),
        vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ]
    );
    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("existing content-type should be kept"),
        "text/custom; charset=utf-8"
    );
    assert_eq!(
        request
            .headers()
            .get("x-extra")
            .expect("custom header should be preserved"),
        "present"
    );
    match request.body() {
        HttpRequestBody::Text(text) => assert_eq!(text.as_str(), "hello"),
        _ => panic!("expected text body"),
    }
}

#[test]
fn test_request_builder_json_body_preserves_existing_content_type() {
    let request = new_builder(Method::POST, "/v1/json")
        .header(CONTENT_TYPE.as_str(), "application/vnd.test+json")
        .expect("custom content-type header should be valid")
        .json_body(&serde_json::json!({ "ok": true }))
        .expect("valid JSON payload should serialize")
        .build();

    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("existing content-type should be kept"),
        "application/vnd.test+json"
    );
    match request.body() {
        HttpRequestBody::Json(bytes) => {
            let body: serde_json::Value = serde_json::from_slice(bytes)
                .expect("JSON body bytes should decode");
            assert_eq!(body["ok"], true);
        }
        _ => panic!("expected JSON body"),
    }
}

#[test]
fn test_request_builder_json_body_serialization_failure_returns_decode_error() {
    let error = new_builder(Method::POST, "/v1/json")
        .json_body(&FailingSerialize)
        .expect_err("failing serializer should return decode error");

    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert!(error.message.contains("Failed to encode JSON body"));
    assert!(!error.to_string().contains("boom"));
    assert!(error
        .source()
        .is_some_and(|source| source.to_string().contains("boom")));
}

#[test]
fn test_request_builder_retry_override_options() {
    let request = new_builder(Method::POST, "/v1/retry")
        .force_retry()
        .retry_method_policy(HttpRetryMethodPolicy::AllMethods)
        .honor_retry_after(true)
        .build();

    assert!(request.retry_override().is_force_enable());
    assert!(!request.retry_override().is_force_disable());
    assert_eq!(
        request.retry_override().method_policy(),
        Some(HttpRetryMethodPolicy::AllMethods)
    );
    assert!(request.retry_override().should_honor_retry_after());
}

#[test]
fn test_request_builder_disable_retry_override() {
    let request = new_builder(http::Method::POST, "/v1/retry-disable")
        .disable_retry()
        .honor_retry_after(false)
        .build();

    assert!(request.retry_override().is_force_disable());
    assert!(!request.retry_override().should_honor_retry_after());
    assert!(request.retry_override().method_policy().is_none());
}

#[test]
fn test_request_builder_sets_cancellation_token() {
    let token = CancellationToken::new();
    let request = new_builder(Method::GET, "/v1/cancel")
        .cancellation_token(token.clone())
        .build();

    assert!(request.cancellation_token().is_some());
    assert!(!request
        .cancellation_token()
        .expect("cancellation token should exist")
        .is_cancelled());
}

#[test]
fn test_request_builder_form_body_sets_content_type_and_encodes_fields() {
    let request = new_builder(Method::POST, "/v1/form")
        .form_body([("name", "alice bob"), ("city", "shanghai")])
        .build();

    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("form body should set Content-Type"),
        "application/x-www-form-urlencoded"
    );
    match request.body() {
        HttpRequestBody::Form(bytes) => {
            let text = String::from_utf8(bytes.to_vec())
                .expect("form payload should be utf-8");
            assert!(text.contains("name=alice+bob"));
            assert!(text.contains("city=shanghai"));
        }
        _ => panic!("expected form body"),
    }
}

#[test]
fn test_request_builder_form_body_preserves_existing_content_type() {
    let request = new_builder(Method::POST, "/v1/form")
        .header(CONTENT_TYPE.as_str(), "application/custom-form")
        .expect("custom content-type header should be valid")
        .form_body([("a", "1")])
        .build();

    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("existing content-type should be kept"),
        "application/custom-form"
    );
}

#[test]
fn test_request_builder_multipart_body_sets_content_type_with_boundary() {
    let request = new_builder(Method::POST, "/v1/multipart")
        .multipart_body(Bytes::from_static(b"--abc\r\n..."), "abc")
        .expect("multipart body should be built")
        .build();

    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("multipart body should set Content-Type"),
        "multipart/form-data; boundary=abc"
    );
    match request.body() {
        HttpRequestBody::Multipart(bytes) => {
            assert_eq!(
                bytes.as_ref(),
                Bytes::from_static(b"--abc\r\n...").as_ref()
            )
        }
        _ => panic!("expected multipart body"),
    }
}

#[test]
fn test_request_builder_multipart_body_rejects_empty_boundary() {
    let error = new_builder(Method::POST, "/v1/multipart")
        .multipart_body(Bytes::from_static(b"payload"), "")
        .expect_err("empty boundary should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("Invalid multipart boundary"));
}

#[test]
fn test_request_builder_multipart_body_rejects_invalid_boundary_header_value() {
    let error = new_builder(Method::POST, "/v1/multipart")
        .multipart_body(Bytes::from_static(b"payload"), "bad\r\nboundary")
        .expect_err("boundary with control chars should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("Invalid multipart boundary"));
}

#[test]
fn test_request_builder_multipart_body_rejects_boundary_with_separator() {
    let error = new_builder(Method::POST, "/v1/multipart")
        .multipart_body(Bytes::from_static(b"payload"), "bad;boundary")
        .expect_err("boundary with content-type separator should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("Invalid multipart boundary"));
}

#[test]
fn test_request_builder_multipart_body_rejects_boundary_with_space() {
    let error = new_builder(Method::POST, "/v1/multipart")
        .multipart_body(Bytes::from_static(b"payload"), "bad boundary")
        .expect_err("boundary with whitespace should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("Invalid multipart boundary"));
}

#[test]
fn test_request_builder_multipart_body_rejects_boundary_with_unquoted_parameter_chars(
) {
    for boundary in [
        "bad/boundary",
        "bad:boundary",
        "bad=boundary",
        "bad?boundary",
        "bad(boundary)",
        "bad,boundary",
    ] {
        let error = new_builder(Method::POST, "/v1/multipart")
            .multipart_body(Bytes::from_static(b"payload"), boundary)
            .expect_err("unquoted Content-Type parameter boundary should reject non-token chars");

        assert_eq!(error.kind, HttpErrorKind::Other);
        assert!(
            error.message.contains("Invalid multipart boundary"),
            "unexpected error for boundary {boundary}: {}",
            error.message
        );
    }
}

#[test]
fn test_request_builder_multipart_body_adds_boundary_to_existing_multipart_content_type(
) {
    let request = new_builder(Method::POST, "/v1/multipart")
        .header(CONTENT_TYPE.as_str(), "multipart/mixed")
        .expect("custom content-type header should be valid")
        .multipart_body(Bytes::from_static(b"payload"), "abc")
        .expect("multipart body should be built")
        .build();

    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("multipart content-type should be kept"),
        "multipart/mixed; boundary=abc"
    );
}

#[test]
fn test_request_builder_multipart_body_preserves_matching_existing_boundary() {
    let request = new_builder(Method::POST, "/v1/multipart")
        .header(CONTENT_TYPE.as_str(), "multipart/mixed; boundary=abc")
        .expect("custom content-type header should be valid")
        .multipart_body(Bytes::from_static(b"payload"), "abc")
        .expect("multipart body should be built")
        .build();

    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("multipart content-type should be kept"),
        "multipart/mixed; boundary=abc"
    );
}

#[test]
fn test_request_builder_multipart_body_preserves_matching_quoted_existing_boundary(
) {
    let request = new_builder(Method::POST, "/v1/multipart")
        .header(CONTENT_TYPE.as_str(), "multipart/mixed; boundary=\"abc\"")
        .expect("custom content-type header should be valid")
        .multipart_body(Bytes::from_static(b"payload"), "abc")
        .expect("quoted multipart boundary should be decoded")
        .build();

    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("multipart content-type should be kept"),
        "multipart/mixed; boundary=\"abc\""
    );
}

#[test]
fn test_request_builder_multipart_body_preserves_matching_escaped_existing_boundary(
) {
    let request = new_builder(Method::POST, "/v1/multipart")
        .header(CONTENT_TYPE.as_str(), "multipart/mixed; boundary=\"ab\\c\"")
        .expect("custom content-type header should be valid")
        .multipart_body(Bytes::from_static(b"payload"), "abc")
        .expect("escaped multipart boundary should be decoded")
        .build();

    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("multipart content-type should be kept"),
        "multipart/mixed; boundary=\"ab\\c\""
    );
}

#[test]
fn test_request_builder_multipart_body_ignores_quoted_boundary_text_in_other_parameters(
) {
    let request = new_builder(Method::POST, "/v1/multipart")
        .header(
            CONTENT_TYPE.as_str(),
            "multipart/mixed; title=\"not; boundary=real\"",
        )
        .expect("custom content-type header should be valid")
        .multipart_body(Bytes::from_static(b"payload"), "abc")
        .expect("missing boundary should be repaired")
        .build();

    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("multipart content-type should be kept"),
        "multipart/mixed; title=\"not; boundary=real\"; boundary=abc"
    );
}

#[test]
fn test_request_builder_multipart_body_ignores_escaped_separator_in_other_parameters(
) {
    let request = new_builder(Method::POST, "/v1/multipart")
        .header(
            CONTENT_TYPE.as_str(),
            "multipart/mixed; title=\"not\\; boundary=real\"",
        )
        .expect("custom content-type header should be valid")
        .multipart_body(Bytes::from_static(b"payload"), "abc")
        .expect("missing boundary should be repaired")
        .build();

    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("multipart content-type should be kept"),
        "multipart/mixed; title=\"not\\; boundary=real\"; boundary=abc"
    );
}

#[test]
fn test_request_builder_multipart_body_skips_parameter_without_value_before_boundary(
) {
    let request = new_builder(Method::POST, "/v1/multipart")
        .header(
            CONTENT_TYPE.as_str(),
            "multipart/mixed; charset; boundary=abc",
        )
        .expect("custom content-type header should be valid")
        .multipart_body(Bytes::from_static(b"payload"), "abc")
        .expect("boundary after value-less parameter should be decoded")
        .build();

    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("multipart content-type should be kept"),
        "multipart/mixed; charset; boundary=abc"
    );
}

#[test]
fn test_request_builder_multipart_body_rejects_boundary_parameter_without_value(
) {
    let error = new_builder(Method::POST, "/v1/multipart")
        .header(CONTENT_TYPE.as_str(), "multipart/mixed; boundary")
        .expect("custom content-type header should be valid")
        .multipart_body(Bytes::from_static(b"payload"), "abc")
        .expect_err("value-less boundary parameter should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("boundary"));
    assert!(error.message.contains("malformed"));
}

#[test]
fn test_request_builder_multipart_body_rejects_mismatched_existing_boundary() {
    let error = new_builder(Method::POST, "/v1/multipart")
        .header(CONTENT_TYPE.as_str(), "multipart/mixed; boundary=other")
        .expect("custom content-type header should be valid")
        .multipart_body(Bytes::from_static(b"payload"), "abc")
        .expect_err("mismatched boundary should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("boundary"));
    assert!(error.message.contains("does not match"));
}

#[test]
fn test_request_builder_multipart_body_rejects_malformed_existing_boundary() {
    let error = new_builder(Method::POST, "/v1/multipart")
        .header(CONTENT_TYPE.as_str(), "multipart/mixed; boundary=\"abc")
        .expect("custom content-type header should be valid")
        .multipart_body(Bytes::from_static(b"payload"), "abc")
        .expect_err("malformed boundary should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("boundary"));
    assert!(error.message.contains("malformed"));
}

#[test]
fn test_request_builder_multipart_body_rejects_boundary_with_trailing_text_after_quote(
) {
    let error = new_builder(Method::POST, "/v1/multipart")
        .header(CONTENT_TYPE.as_str(), "multipart/mixed; boundary=\"abc\"x")
        .expect("custom content-type header should be valid")
        .multipart_body(Bytes::from_static(b"payload"), "abc")
        .expect_err("quoted boundary with trailing text should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("boundary"));
    assert!(error.message.contains("malformed"));
}

#[test]
fn test_request_builder_multipart_body_rejects_existing_non_multipart_content_type(
) {
    let error = new_builder(Method::POST, "/v1/multipart")
        .header(CONTENT_TYPE.as_str(), "application/octet-stream")
        .expect("custom content-type header should be valid")
        .multipart_body(Bytes::from_static(b"payload"), "abc")
        .expect_err("non-multipart content-type should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("Content-Type"));
    assert!(error.message.contains("multipart"));
}

#[test]
fn test_request_builder_multipart_body_rejects_non_utf8_existing_content_type()
{
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_bytes(b"multipart/mixed; boundary=\xFF").expect(
            "non-UTF-8 header value should be accepted as raw header bytes",
        ),
    );

    let error = new_builder(Method::POST, "/v1/multipart")
        .headers(headers)
        .multipart_body(Bytes::from_static(b"payload"), "abc")
        .expect_err("non-UTF-8 content-type should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("Content-Type"));
    assert!(error.message.contains("valid UTF-8"));
}

#[test]
fn test_request_builder_ndjson_body_sets_content_type_and_serializes_lines() {
    #[derive(serde::Serialize)]
    struct Record {
        id: i32,
    }

    let request = new_builder(Method::POST, "/v1/ndjson")
        .ndjson_body(&[Record { id: 1 }, Record { id: 2 }])
        .expect("ndjson should be encoded")
        .build();

    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("ndjson body should set Content-Type"),
        "application/x-ndjson"
    );
    match request.body() {
        HttpRequestBody::Ndjson(bytes) => {
            let text = String::from_utf8(bytes.to_vec())
                .expect("ndjson payload should be utf-8");
            assert_eq!(text, "{\"id\":1}\n{\"id\":2}\n");
        }
        _ => panic!("expected ndjson body"),
    }
}

#[test]
fn test_request_builder_ndjson_body_serialization_failure_returns_decode_error()
{
    let records = [FailingSerialize];
    let error = new_builder(Method::POST, "/v1/ndjson")
        .ndjson_body(&records)
        .expect_err("failing serializer should return decode error");

    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert!(error.message.contains("Failed to encode NDJSON record"));
    assert!(!error.to_string().contains("boom"));
    assert!(error
        .source()
        .is_some_and(|source| source.to_string().contains("boom")));
}

#[test]
fn test_request_builder_ndjson_body_preserves_existing_content_type() {
    #[derive(serde::Serialize)]
    struct Record {
        id: i32,
    }

    let request = new_builder(Method::POST, "/v1/ndjson")
        .header(CONTENT_TYPE.as_str(), "application/custom-ndjson")
        .expect("custom content-type header should be valid")
        .ndjson_body(&[Record { id: 9 }])
        .expect("ndjson should be encoded")
        .build();

    assert_eq!(
        request
            .headers()
            .get(CONTENT_TYPE)
            .expect("existing content-type should be kept"),
        "application/custom-ndjson"
    );
}

#[test]
fn test_request_builder_ndjson_body_allows_empty_records() {
    #[derive(serde::Serialize)]
    struct Record {
        id: i32,
    }

    let records: [Record; 0] = [];
    let request = new_builder(Method::POST, "/v1/ndjson")
        .ndjson_body(&records)
        .expect("empty ndjson records should be encoded")
        .build();

    match request.body() {
        HttpRequestBody::Ndjson(bytes) => assert!(bytes.is_empty()),
        _ => panic!("expected ndjson body"),
    }
}
