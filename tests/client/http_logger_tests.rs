// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use bytes::Bytes;
use http::HeaderMap;
use http::Method;
use qubit_http::HttpClientFactory;
use qubit_http::HttpClientOptions;
use qubit_http::HttpLogger;
use qubit_http::HttpLoggingOptions;
use qubit_redact::InputOutputLimit;
use qubit_redact::RedactionPolicy;
use qubit_redact::http::HttpRedactor;
use qubit_redact::http::TextBodyPolicy;
use url::Url;

use crate::common::capture_trace_logs;

#[test]
fn test_http_logger_logs_request_body_preview_with_truncation() {
    let mut options = HttpClientOptions::default();
    options.logging = HttpLoggingOptions {
        log_request_header: false,
        log_request_body: true,
        body_size_limit: 4,
        ..HttpLoggingOptions::default()
    };
    options.log_redaction_policy = RedactionPolicy::default()
        .to_builder()
        .text_body_policy(TextBodyPolicy::PassThrough)
        .build()
        .expect("log redaction policy should be valid");
    let logger = HttpLogger::new(&options);
    let client = HttpClientFactory::new()
        .create_default()
        .expect("default client should be created");
    let request = client
        .request(Method::POST, "https://example.com/upload")
        .text_body("abcdef")
        .build();

    let logs = capture_trace_logs(|| logger.log_request(&request));

    assert!(logs.contains("--> POST https://example.com/upload"));
    assert!(logs.contains("Request body: abcd<truncated>"));
}

#[test]
fn test_http_logger_redacts_request_url_query_and_json_body() {
    let options = HttpClientOptions::default();
    let logger = HttpLogger::new(&options);
    let client = HttpClientFactory::new()
        .create_default()
        .expect("default client should be created");
    let request = client
        .request(
            Method::POST,
            "https://example.com/login?access_token=raw-token",
        )
        .json_body(&serde_json::json!({
            "user": "alice",
            "password": "secret",
        }))
        .expect("JSON body should serialize")
        .build();

    let logs = capture_trace_logs(|| logger.log_request(&request));

    assert!(logs.contains("--> POST https://example.com/login?access_token=****"));
    assert!(logs.contains(r#""password":"<redacted>""#));
    assert!(!logs.contains("raw-token"));
    assert!(!logs.contains("secret"));
}

#[test]
fn test_http_logger_does_not_leak_multipart_body_sensitive_values() {
    let options = HttpClientOptions::default();
    let logger = HttpLogger::new(&options);
    let client = HttpClientFactory::new()
        .create_default()
        .expect("default client should be created");
    let body = Bytes::from_static(
        b"--boundary\r\nContent-Disposition: form-data; name=\"password\"\r\n\r\nsecret-password\r\n--boundary--",
    );
    let request = client
        .request(Method::POST, "https://example.com/upload")
        .multipart_body(body, "boundary")
        .expect("multipart body should be accepted")
        .build();

    let logs = capture_trace_logs(|| logger.log_request(&request));

    assert!(logs.contains("password=<redacted>"));
    assert!(!logs.contains("secret-password"));
    assert!(!logs.contains("--boundary"));
}

#[test]
fn test_http_logger_does_not_leak_multipart_mixed_body_sensitive_values() {
    let options = HttpClientOptions::default();
    let logger = HttpLogger::new(&options);
    let client = HttpClientFactory::new()
        .create_default()
        .expect("default client should be created");
    let body = Bytes::from_static(
        b"--boundary\r\nContent-Disposition: form-data; name=\"password\"\r\n\r\nsecret-password\r\n--boundary--",
    );
    let request = client
        .request(Method::POST, "https://example.com/upload")
        .header("content-type", "multipart/mixed; boundary=boundary")
        .expect("custom content-type should be accepted")
        .multipart_body(body, "boundary")
        .expect("multipart body should be accepted")
        .build();

    let logs = capture_trace_logs(|| logger.log_request(&request));

    assert!(logs.contains("password=<redacted>"));
    assert!(!logs.contains("secret-password"));
    assert!(!logs.contains("--boundary"));
}

#[test]
fn test_http_redaction_session_shares_output_exhaustion_without_input_charge() {
    let budget = InputOutputLimit::new(4096, InputOutputLimit::MIN_OUTPUT_BYTES)
        .expect("diagnostic budget should be valid");
    let policy = RedactionPolicy::default()
        .to_builder()
        .diagnostic_event(budget)
        .build()
        .expect("policy should be valid");
    let redactor = HttpRedactor::new(policy);
    let mut session = redactor.session();
    let input_before = session.remaining_input_bytes();
    let url = Url::parse(&format!(
        "https://example.com/{}",
        "path".repeat(InputOutputLimit::MIN_OUTPUT_BYTES),
    ))
    .expect("URL should parse");

    let first = session.http().redact_url(&url);
    assert_eq!(first.as_str().len(), InputOutputLimit::MIN_OUTPUT_BYTES);
    assert!(first.as_str().ends_with("<truncated>"));
    assert!(session.is_exhausted());
    let input_after_first = session.remaining_input_bytes();
    assert!(input_after_first < input_before);

    let second = session.http().redact_headers(&HeaderMap::new());
    assert!(second.log_safe_text().as_str().is_empty());
    assert_eq!(session.remaining_input_bytes(), input_after_first);

    let third = session
        .http()
        .redact_body(qubit_redact::http::BodyCapture::complete(b"body"), None);
    assert!(third.log_safe_text().as_str().is_empty());
    assert_eq!(session.remaining_input_bytes(), input_after_first);
}
