// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use bytes::Bytes;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use qubit_http::HttpClientFactory;
use qubit_http::HttpClientOptions;
use qubit_http::HttpLogger;
use qubit_http::HttpLoggingOptions;
use qubit_redact::InputOutputLimit;
use qubit_redact::RedactionCompletion;
use qubit_redact::RedactionPolicy;
use qubit_redact::formats::http::BodyCapture;
use qubit_redact::formats::http::BodyRedactionStatus;
use qubit_redact::formats::http::HttpRedactor;
use qubit_redact::formats::http::TextBodyPolicy;
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
    let mut policy_builder = RedactionPolicy::default().to_builder();
    policy_builder.http().text_body(TextBodyPolicy::PassThrough);
    options.log_redaction_policy = policy_builder
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
fn test_http_logger_maps_exhausted_body_completion_to_outer_marker() {
    let budget = InputOutputLimit::builder()
        .max_input_bytes(4096)
        .max_output_bytes(InputOutputLimit::MIN_OUTPUT_BYTES)
        .build()
        .expect("diagnostic budget should be valid");
    let mut policy_builder = RedactionPolicy::default().to_builder();
    policy_builder.limits().diagnostic_event(budget);
    policy_builder.http().text_body(TextBodyPolicy::PassThrough);
    let mut options = HttpClientOptions::default();
    options.logging.log_request_header = false;
    options.log_redaction_policy = policy_builder
        .build()
        .expect("log redaction policy should be valid");
    let logger = HttpLogger::new(&options);
    let client = HttpClientFactory::new()
        .create_default()
        .expect("default client should be created");
    let url = format!(
        "https://example.com/{}",
        "path".repeat(InputOutputLimit::MIN_OUTPUT_BYTES),
    );
    let request = client.request(Method::POST, &url).text_body("body").build();

    let logs = capture_trace_logs(|| logger.log_request(&request));

    assert!(logs.contains("Request body: <truncated>"));
}

#[test]
fn test_http_body_redaction_reports_complete_for_normal_content() {
    let redaction = HttpRedactor::default().redact_body(
        BodyCapture::complete(br#"{"visible":"ok"}"#),
        Some(&HeaderValue::from_static("application/json")),
    );

    assert_eq!(redaction.status(), BodyRedactionStatus::Structured);
    assert_eq!(redaction.completion(), RedactionCompletion::Complete);
}

#[test]
fn test_http_body_redaction_reports_truncated_for_bounded_preview() {
    let mut policy_builder = RedactionPolicy::default().to_builder();
    policy_builder.http().text_body(TextBodyPolicy::PassThrough);
    let policy = policy_builder
        .build()
        .expect("log redaction policy should be valid");
    let redaction = HttpRedactor::new(policy).redact_body(
        BodyCapture::prefix(b"abcdef", 4),
        Some(&HeaderValue::from_static("text/plain")),
    );

    assert_eq!(redaction.status(), BodyRedactionStatus::PassedThrough);
    assert_eq!(redaction.completion(), RedactionCompletion::Truncated);
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

    assert!(
        logs.contains("--> POST https://example.com/login?access_token=****")
    );
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
    let budget = InputOutputLimit::builder()
        .max_input_bytes(4096)
        .max_output_bytes(InputOutputLimit::MIN_OUTPUT_BYTES)
        .build()
        .expect("diagnostic budget should be valid");
    let mut policy_builder = RedactionPolicy::default().to_builder();
    policy_builder.limits().diagnostic_event(budget);
    let policy = policy_builder.build().expect("policy should be valid");
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
        .redact_body(BodyCapture::complete(b"body"), None);
    assert_eq!(third.completion(), RedactionCompletion::Exhausted);
    assert!(third.log_safe_text().as_str().is_empty());
    assert_eq!(session.remaining_input_bytes(), input_after_first);
}
