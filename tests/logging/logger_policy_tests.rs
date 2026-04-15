/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use bytes::Bytes;
use http::header::{AUTHORIZATION, CONTENT_TYPE, SET_COOKIE};
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use qubit_http::{
    HttpClientFactory, HttpClientOptions, HttpLogger, HttpLoggingOptions, HttpResponse,
    HttpRequest, HttpRequestBody, HttpResponseMeta, SensitiveHeaders,
};
use url::Url;

use crate::common::capture_trace_logs;

fn logging_request(method: Method, path: &str, headers: HeaderMap, body: HttpRequestBody) -> HttpRequest {
    let client = HttpClientFactory::new()
        .create()
        .expect("default options should create client");
    let base = client.request(method, path).headers(headers);
    match body {
        HttpRequestBody::Empty => base.build(),
        HttpRequestBody::Text(text) => base.text_body(text).build(),
        HttpRequestBody::Json(bytes) | HttpRequestBody::Bytes(bytes) => base.bytes_body(bytes).build(),
        HttpRequestBody::Stream(chunks) => base.stream_body(chunks).build(),
        other => panic!("logging_request: unsupported body variant: {:?}", other),
    }
}

#[test]
fn test_log_request_disabled_emits_nothing() {
    let mut options = HttpLoggingOptions::default();
    options.enabled = false;
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let sensitive_headers = SensitiveHeaders::default();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    client_options.sensitive_headers = sensitive_headers;
    let logger = HttpLogger::new(&client_options);

    let request = logging_request(
        Method::POST,
        "https://example.com/api",
        headers,
        HttpRequestBody::Json(Bytes::from_static(br#"{"x":1}"#)),
    );
    let logs = capture_trace_logs(|| {
        logger.log_request(&request);
    });
    assert!(logs.trim().is_empty());
}

#[test]
fn test_log_request_toggles_header_and_body() {
    let mut options = HttpLoggingOptions::default();
    options.log_request_header = false;
    options.log_request_body = false;

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let sensitive_headers = SensitiveHeaders::default();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    client_options.sensitive_headers = sensitive_headers;
    let logger = HttpLogger::new(&client_options);

    let request = logging_request(
        Method::POST,
        "https://example.com/api",
        headers,
        HttpRequestBody::Json(Bytes::from_static(br#"{"x":1}"#)),
    );
    let logs = capture_trace_logs(|| {
        logger.log_request(&request);
    });
    assert!(logs.contains("--> POST https://example.com/api"));
    assert!(!logs.contains("application/json"));
    assert!(!logs.contains("Request body:"));
}

#[test]
fn test_log_response_masks_sensitive_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, HeaderValue::from_static("session-token-value"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer very-secret-token"),
    );
    let options = HttpLoggingOptions::default();
    let sensitive_headers = SensitiveHeaders::default();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    client_options.sensitive_headers = sensitive_headers;
    let logger = HttpLogger::new(&client_options);

    let logs = capture_trace_logs(|| {
        let response = HttpResponse::new(
            StatusCode::OK,
            headers.clone(),
            Bytes::from_static(b"ok"),
            Url::parse("https://example.com/data").unwrap(),
            Method::GET,
        );
        logger.log_response(&response);
    });
    assert!(logs.contains("set-cookie: se****ue"));
    assert!(logs.contains("authorization: Be****en"));
}

#[test]
fn test_log_response_binary_body_and_truncation() {
    let options = HttpLoggingOptions {
        body_size_limit: 4,
        ..HttpLoggingOptions::default()
    };
    let headers = HeaderMap::new();
    let sensitive_headers = SensitiveHeaders::default();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    client_options.sensitive_headers = sensitive_headers;
    let logger = HttpLogger::new(&client_options);

    let logs = capture_trace_logs(|| {
        let response = HttpResponse::new(
            StatusCode::OK,
            headers.clone(),
            Bytes::from_static(&[0xFF, 0xFE, 0xFD, 0xFC, 0xFB]),
            Url::parse("https://example.com/bin").unwrap(),
            Method::GET,
        );
        logger.log_response(&response);
    });
    assert!(logs.contains("Response body: <binary 5 bytes>...<truncated 1 bytes>"));
}

#[test]
fn test_log_stream_response_headers_respects_toggle() {
    let mut options = HttpLoggingOptions::default();
    options.log_response_header = false;
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    let sensitive_headers = SensitiveHeaders::default();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    client_options.sensitive_headers = sensitive_headers;
    let logger = HttpLogger::new(&client_options);

    let logs = capture_trace_logs(|| {
        let response_meta = HttpResponseMeta::new(
            StatusCode::OK,
            headers.clone(),
            Url::parse("https://example.com/stream").unwrap(),
            Method::GET,
        );
        logger.log_stream_response_headers(&response_meta);
    });
    assert!(logs.contains("<-- 200 https://example.com/stream (stream)"));
    assert!(!logs.contains("text/event-stream"));
}

#[test]
fn test_log_request_text_body() {
    let options = HttpLoggingOptions::default();
    let headers = HeaderMap::new();
    let sensitive_headers = SensitiveHeaders::default();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    client_options.sensitive_headers = sensitive_headers;
    let logger = HttpLogger::new(&client_options);

    let request = logging_request(
        Method::POST,
        "https://example.com/text",
        headers,
        HttpRequestBody::Text("hello body".to_string()),
    );
    let logs = capture_trace_logs(|| {
        logger.log_request(&request);
    });
    assert!(logs.contains("--> POST https://example.com/text"));
    assert!(logs.contains("Request body: hello body"));
}

#[test]
fn test_log_request_stream_body_logged_as_empty() {
    let options = HttpLoggingOptions::default();
    let headers = HeaderMap::new();
    let sensitive_headers = SensitiveHeaders::default();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    client_options.sensitive_headers = sensitive_headers;
    let logger = HttpLogger::new(&client_options);

    let request = logging_request(
        Method::POST,
        "https://example.com/stream-upload",
        headers,
        HttpRequestBody::Stream(vec![Bytes::from_static(b"a"), Bytes::from_static(b"b")]),
    );
    let logs = capture_trace_logs(|| {
        logger.log_request(&request);
    });
    assert!(logs.contains("--> POST https://example.com/stream-upload"));
    assert!(logs.contains("Request body: <empty>"));
}
