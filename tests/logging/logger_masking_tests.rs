/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use http::header::{HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use http::{HeaderMap, Method};
use qubit_http::{
    HttpClientFactory, HttpClientOptions, HttpLogger, HttpLoggingOptions, SensitiveHttpHeaders,
};

use crate::common::capture_trace_logs;

fn capture_request_header_logs(name: HeaderName, value: HeaderValue) -> String {
    let options = HttpLoggingOptions {
        log_request_header: true,
        log_request_body: false,
        ..HttpLoggingOptions::default()
    };
    let sensitive_headers = SensitiveHttpHeaders::default();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    client_options.sensitive_headers = sensitive_headers;
    let logger = HttpLogger::new(&client_options);
    let mut headers = HeaderMap::new();
    headers.insert(name, value);

    let client = HttpClientFactory::new()
        .create_default()
        .expect("default options should create client");
    let request = client
        .request(Method::GET, "https://example.com/")
        .headers(headers)
        .build();

    capture_trace_logs(|| {
        logger.log_request(&request);
    })
}

#[test]
fn test_mask_header_value_non_sensitive_header() {
    let logs =
        capture_request_header_logs(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    assert!(logs.contains("content-type: application/json"));
}

#[test]
fn test_mask_header_value_sensitive_short_value() {
    let logs = capture_request_header_logs(AUTHORIZATION, HeaderValue::from_static("abc"));
    assert!(logs.contains("authorization: ****"));
}

#[test]
fn test_mask_header_value_sensitive_exactly_four_chars() {
    let logs = capture_request_header_logs(AUTHORIZATION, HeaderValue::from_static("abcd"));
    assert!(logs.contains("authorization: ****"));
}

#[test]
fn test_mask_header_value_sensitive_long_value() {
    let logs = capture_request_header_logs(AUTHORIZATION, HeaderValue::from_static("abcdefghijk"));
    assert!(logs.contains("authorization: ab****jk"));
}

#[test]
fn test_mask_header_value_sensitive_case_insensitive() {
    let logs = capture_request_header_logs(
        HeaderName::from_static("x-api-key"),
        HeaderValue::from_static("1234567890"),
    );
    assert!(logs.contains("x-api-key: 12****90"));
}

#[test]
fn test_mask_header_value_empty_value_kept_empty() {
    let logs = capture_request_header_logs(AUTHORIZATION, HeaderValue::from_static(""));
    assert!(logs.contains("authorization: "));
}
