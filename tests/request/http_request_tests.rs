/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::time::Duration;

use bytes::Bytes;
use futures_util::stream;
use http::{HeaderName, HeaderValue, Method};
use qubit_http::{
    CancellationToken, HttpClientFactory, HttpClientOptions, HttpErrorKind, HttpRequestBody,
    HttpRequestBodyByteStream, HttpRequestRetryOverride, HttpRequestStreamingBody,
    HttpRetryMethodPolicy,
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
        HttpRequestBody::Bytes(bytes) => assert_eq!(bytes, &Bytes::from_static(b"payload")),
        _ => panic!("expected bytes body"),
    }
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
    request.set_request_timeout(Duration::from_secs(5));
    assert_eq!(request.request_timeout(), Some(Duration::from_secs(5)));
    request.clear_request_timeout();
    assert_eq!(request.request_timeout(), None);

    request
        .set_write_timeout(Duration::from_millis(250))
        .set_read_timeout(Duration::from_millis(750));
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
fn test_http_request_setters_refresh_resolved_url_cache_for_base_url_and_ipv4_only() {
    let mut options = HttpClientOptions::default();
    options
        .set_base_url("https://api.example.com/v1/")
        .expect("base URL should parse");

    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let mut request = client.request(Method::GET, "users").build();

    assert_eq!(
        request.resolved_url_cached().as_ref().map(Url::as_str),
        Some("https://api.example.com/v1/users")
    );

    request.set_path("orders");
    assert_eq!(
        request.resolved_url_cached().as_ref().map(Url::as_str),
        Some("https://api.example.com/v1/orders")
    );

    request.clear_base_url();
    assert!(request.resolved_url_cached().is_none());

    request.set_base_url(Url::parse("https://api.example.com/v2/").unwrap());
    assert_eq!(
        request.resolved_url_cached().as_ref().map(Url::as_str),
        Some("https://api.example.com/v2/orders")
    );

    request.set_ipv4_only(true).set_path("http://[::1]/ipv6");
    assert!(request.resolved_url_cached().is_none());

    request.set_ipv4_only(false);
    assert_eq!(
        request.resolved_url_cached().as_ref().map(Url::as_str),
        Some("http://[::1]/ipv6")
    );
}

#[test]
fn test_http_request_set_streaming_body_replaces_existing_body_and_has_safe_debug() {
    let mut request = new_request(Method::POST, "/streaming-upload");
    request.set_body(HttpRequestBody::Bytes(Bytes::from_static(b"legacy-body")));

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
