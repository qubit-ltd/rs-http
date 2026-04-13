/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use http::header::{HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use http::{HeaderMap, Method};
use qubit_http::{HttpLogger, HttpLoggingOptions, SensitiveHeaders};
use url::Url;

use crate::common::capture_trace_logs;

fn capture_request_header_logs(name: HeaderName, value: HeaderValue) -> String {
    let options = HttpLoggingOptions {
        log_request_header: true,
        log_request_body: false,
        ..HttpLoggingOptions::default()
    };
    let sensitive_headers = SensitiveHeaders::default();
    let logger = HttpLogger::new(&options, &sensitive_headers);
    let mut headers = HeaderMap::new();
    headers.insert(name, value);

    capture_trace_logs(|| {
        logger.log_request(
            &Method::GET,
            &Url::parse("https://example.com/").unwrap(),
            &headers,
            None,
        );
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
