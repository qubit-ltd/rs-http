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
use qubit_http::logging::{log_request, log_response, log_stream_response_headers};
use qubit_http::{HttpLoggingOptions, SensitiveHeaders};
use url::Url;

use crate::common::capture_trace_logs;

#[test]
fn test_log_request_disabled_emits_nothing() {
    let mut options = HttpLoggingOptions::default();
    options.enabled = false;
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    let logs = capture_trace_logs(|| {
        log_request(
            &Method::POST,
            &Url::parse("https://example.com/api").unwrap(),
            &headers,
            Some(&Bytes::from_static(br#"{"x":1}"#)),
            &options,
            &SensitiveHeaders::default(),
        );
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

    let logs = capture_trace_logs(|| {
        log_request(
            &Method::POST,
            &Url::parse("https://example.com/api").unwrap(),
            &headers,
            Some(&Bytes::from_static(br#"{"x":1}"#)),
            &options,
            &SensitiveHeaders::default(),
        );
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

    let logs = capture_trace_logs(|| {
        log_response(
            StatusCode::OK,
            &Url::parse("https://example.com/data").unwrap(),
            &headers,
            &Bytes::from_static(b"ok"),
            &HttpLoggingOptions::default(),
            &SensitiveHeaders::default(),
        );
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

    let logs = capture_trace_logs(|| {
        log_response(
            StatusCode::OK,
            &Url::parse("https://example.com/bin").unwrap(),
            &headers,
            &Bytes::from_static(&[0xFF, 0xFE, 0xFD, 0xFC, 0xFB]),
            &options,
            &SensitiveHeaders::default(),
        );
    });
    assert!(logs.contains("Response body: <binary 5 bytes>...<truncated 1 bytes>"));
}

#[test]
fn test_log_stream_response_headers_respects_toggle() {
    let mut options = HttpLoggingOptions::default();
    options.log_response_header = false;
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));

    let logs = capture_trace_logs(|| {
        log_stream_response_headers(
            StatusCode::OK,
            &Url::parse("https://example.com/stream").unwrap(),
            &headers,
            &options,
            &SensitiveHeaders::default(),
        );
    });
    assert!(logs.contains("<-- 200 https://example.com/stream (stream)"));
    assert!(!logs.contains("text/event-stream"));
}
