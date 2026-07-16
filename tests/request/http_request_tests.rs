// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use bytes::Bytes;
use futures_util::stream;
use http::{
    HeaderName,
    HeaderValue,
    Method,
};
use qubit_http::{
    CancellationToken,
    HttpClientFactory,
    HttpClientOptions,
    HttpErrorKind,
    HttpRequestBody,
    HttpRequestBodyByteStream,
    HttpRequestRetryOverride,
    HttpRequestStreamingBody,
    HttpRetryMethodPolicy,
    LogSanitizePolicy,
    UrlPathPolicy,
};
use url::Url;

fn new_request(method: Method, path: &str) -> qubit_http::HttpRequest {
    let client = HttpClientFactory::new()
        .create_default()
        .expect("default options should create client");
    client.request(method, path).build()
}

#[test]
fn test_http_request_setters_update_method_path_query_and_body() {
    let mut request = new_request(Method::GET, "/v1/items");
    assert_eq!(request.method(), &Method::GET);
    assert_eq!(request.path(), "/v1/items");

    request.set_method(Method::POST).set_path("/v2/orders");
    assert_eq!(request.method(), &Method::POST);
    assert_eq!(request.path(), "/v2/orders");

    request
        .add_query_param("page", "1")
        .add_query_param("limit", "10");
    assert_eq!(
        request.query(),
        vec![
            ("page".to_string(), "1".to_string()),
            ("limit".to_string(), "10".to_string()),
        ]
    );
    request.clear_query_params();
    assert!(request.query().is_empty());

    request.set_body(HttpRequestBody::Bytes(Bytes::from_static(b"payload")));
    match request.body() {
        HttpRequestBody::Bytes(bytes) => {
            assert_eq!(bytes, &Bytes::from_static(b"payload"))
        }
        _ => panic!("expected bytes body"),
    }
}

#[test]
fn test_http_request_debug_masks_sensitive_values() {
    let client = HttpClientFactory::new()
        .create_default()
        .expect("default options should create client");
    let request = client
        .request(
            Method::POST,
            "https://debug-user:debug-url-secret@example.com/v1?access_token=debug-query-secret#debug-fragment-secret",
        )
        .header("authorization", "Bearer debug-header-secret")
        .expect("authorization header should be accepted")
        .json_body(&serde_json::json!({
            "password": "debug-body-secret",
            "user": "alice"
        }))
        .expect("json body should serialize")
        .build();
    let mut options = HttpClientOptions::new();
    options
        .set_base_url("https://debug-user:debug-url-secret@example.com/root/?accessToken=debug-base-query-secret")
        .expect("base URL should be valid");
    let relative_client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let relative_request = relative_client
        .request(Method::GET, "items")
        .query_param("clientSecret", "debug-added-query-secret")
        .build();

    let debug = format!("{request:?}\n{relative_request:?}");

    assert!(!debug.contains("debug-user"));
    assert!(!debug.contains("debug-url-secret"));
    assert!(!debug.contains("debug-query-secret"));
    assert!(!debug.contains("debug-base-query-secret"));
    assert!(!debug.contains("debug-added-query-secret"));
    assert!(!debug.contains("debug-fragment-secret"));
    assert!(!debug.contains("debug-header-secret"));
    assert!(!debug.contains("debug-body-secret"));
    assert!(debug.contains("****"));
}

#[test]
fn test_http_request_debug_honors_url_path_redaction_policy() {
    let mut options = HttpClientOptions::new();
    options.log_sanitize_policy = LogSanitizePolicy::default()
        .with_url_path_policy(UrlPathPolicy::Redact);
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let request = client
        .request(
            Method::GET,
            "https://alice:request-password-secret@example.com/tenant/request-path-secret?access_token=request-query-secret#request-fragment-secret",
        )
        .build();

    let debug = format!("{request:?}");

    assert!(!debug.contains("tenant/request-path-secret"));
    assert!(!debug.contains("alice"));
    assert!(!debug.contains("request-password-secret"));
    assert!(!debug.contains("request-query-secret"));
    assert!(!debug.contains("request-fragment-secret"));
    assert!(debug.contains("/%3Credacted%3E?"));
}

#[test]
fn test_http_request_resolved_url_is_public() {
    let mut options = HttpClientOptions::new();
    options
        .set_base_url("https://api.example.com/root/")
        .expect("base URL should be valid");
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let request = client
        .request(Method::GET, "items?existing=1")
        .query_param("added", "two words")
        .build();

    let url = request
        .resolved_url()
        .expect("resolved request URL should be public");

    assert_eq!(
        url.as_str(),
        "https://api.example.com/root/items?existing=1&added=two+words"
    );
}

#[test]
fn test_http_request_setters_update_headers_timeout_retry_and_cancellation() {
    let mut request = new_request(Method::GET, "/v1/resources");

    request
        .set_header("x-trace-id", "trace-1")
        .expect("valid header should be accepted");
    request.set_typed_header(
        HeaderName::from_static("x-role"),
        HeaderValue::from_static("tester"),
    );
    assert_eq!(
        request
            .headers()
            .get("x-trace-id")
            .expect("x-trace-id header should exist"),
        "trace-1"
    );
    assert_eq!(
        request
            .headers()
            .get("x-role")
            .expect("x-role header should exist"),
        "tester"
    );

    request.remove_header(&HeaderName::from_static("x-trace-id"));
    assert!(!request.headers().contains_key("x-trace-id"));

    let error = request
        .set_header("invalid header", "value")
        .expect_err("invalid header name should fail");
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(!request.headers().contains_key("invalid header"));

    request.clear_headers();
    assert!(request.headers().is_empty());

    assert_eq!(request.request_timeout(), None);
    request
        .set_request_timeout(Duration::from_secs(5))
        .expect("positive request timeout should be accepted");
    assert_eq!(request.request_timeout(), Some(Duration::from_secs(5)));
    request.clear_request_timeout();
    assert_eq!(request.request_timeout(), None);

    request
        .set_write_timeout(Duration::from_millis(250))
        .expect("positive write timeout should be accepted");
    request
        .set_read_timeout(Duration::from_millis(750))
        .expect("positive read timeout should be accepted");
    assert_eq!(request.write_timeout(), Duration::from_millis(250));
    assert_eq!(request.read_timeout(), Duration::from_millis(750));

    let token = CancellationToken::new();
    request.set_cancellation_token(token.clone());
    assert!(request.cancellation_token().is_some());
    request.clear_cancellation_token();
    assert!(request.cancellation_token().is_none());

    let retry_override = HttpRequestRetryOverride::new()
        .force_enable()
        .with_method_policy(HttpRetryMethodPolicy::AllMethods)
        .with_honor_retry_after(true);
    request.set_retry_override(retry_override.clone());
    assert_eq!(request.retry_override(), &retry_override);
}

#[test]
fn test_http_request_timeout_setters_reject_zero_and_keep_previous_values() {
    let mut request = new_request(Method::GET, "/v1/resources");
    request
        .set_request_timeout(Duration::from_secs(5))
        .expect("positive request timeout should be accepted");
    request
        .set_write_timeout(Duration::from_millis(250))
        .expect("positive write timeout should be accepted");
    request
        .set_read_timeout(Duration::from_millis(750))
        .expect("positive read timeout should be accepted");

    let request_timeout_error = request
        .set_request_timeout(Duration::ZERO)
        .expect_err("zero request timeout should be rejected");
    assert_eq!(request_timeout_error.kind, HttpErrorKind::Other);
    assert!(request_timeout_error.message.contains("request_timeout"));
    assert_eq!(request.request_timeout(), Some(Duration::from_secs(5)));

    let write_timeout_error = request
        .set_write_timeout(Duration::ZERO)
        .expect_err("zero write timeout should be rejected");
    assert_eq!(write_timeout_error.kind, HttpErrorKind::Other);
    assert!(write_timeout_error.message.contains("write_timeout"));
    assert_eq!(request.write_timeout(), Duration::from_millis(250));

    let read_timeout_error = request
        .set_read_timeout(Duration::ZERO)
        .expect_err("zero read timeout should be rejected");
    assert_eq!(read_timeout_error.kind, HttpErrorKind::Other);
    assert!(read_timeout_error.message.contains("read_timeout"));
    assert_eq!(request.read_timeout(), Duration::from_millis(750));
}

#[test]
fn test_http_request_setters_update_resolved_url_for_base_url_and_ipv4_only() {
    let mut options = HttpClientOptions::default();
    options
        .set_base_url("https://api.example.com/v1/")
        .expect("base URL should parse");

    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let mut request = client.request(Method::GET, "users").build();

    assert_eq!(
        request
            .resolved_url()
            .expect("request URL should resolve")
            .as_str(),
        "https://api.example.com/v1/users"
    );

    request.set_path("orders");
    assert_eq!(
        request
            .resolved_url()
            .expect("request URL should resolve after path change")
            .as_str(),
        "https://api.example.com/v1/orders"
    );

    request.clear_base_url();
    assert!(request.resolved_url().is_err());

    request.set_base_url(Url::parse("https://api.example.com/v2/").unwrap());
    assert_eq!(
        request
            .resolved_url()
            .expect("request URL should resolve after base URL change")
            .as_str(),
        "https://api.example.com/v2/orders"
    );

    request.set_ipv4_only(true).set_path("http://[::1]/ipv6");
    assert!(request.resolved_url().is_err());

    request.set_ipv4_only(false);
    assert_eq!(
        request
            .resolved_url()
            .expect("IPv6 URL should resolve after ipv4_only is disabled")
            .as_str(),
        "http://[::1]/ipv6"
    );
}

#[test]
fn test_http_request_set_streaming_body_replaces_existing_body_and_has_safe_debug(
) {
    let mut request = new_request(Method::POST, "/streaming-upload");
    request
        .set_body(HttpRequestBody::Bytes(Bytes::from_static(b"legacy-body")));

    let streaming_body = HttpRequestStreamingBody::new(|| {
        Box::pin(async move {
            let source = stream::iter(vec![
                Ok(Bytes::from_static(b"chunk-1")),
                Ok(Bytes::from_static(b"chunk-2")),
            ]);
            Box::pin(source) as HttpRequestBodyByteStream
        })
    });
    let debug = format!("{streaming_body:?}");
    assert!(debug.contains("HttpRequestStreamingBody"));

    request.set_streaming_body(streaming_body);
    assert_eq!(request.body(), &HttpRequestBody::Empty);
}
