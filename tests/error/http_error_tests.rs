/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use http::StatusCode;
use qubit_http::{HttpError, HttpErrorKind, RetryHint};

#[test]
fn test_http_error_builder_methods() {
    let url = url::Url::parse("https://example.com/test").unwrap();
    let error = HttpError::new(HttpErrorKind::Decode, "decode failure")
        .with_method(&http::Method::POST)
        .with_url(&url)
        .with_status(StatusCode::BAD_GATEWAY);

    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert_eq!(error.method, Some(http::Method::POST));
    assert_eq!(error.url, Some(url));
    assert_eq!(error.status, Some(StatusCode::BAD_GATEWAY));
    assert!(error.message.contains("decode failure"));
}

#[test]
fn test_http_error_build_client_constructor() {
    let error = HttpError::build_client("builder failed");
    assert_eq!(error.kind, HttpErrorKind::BuildClient);
    assert_eq!(error.message, "builder failed");
}

#[test]
fn test_http_error_request_timeout_constructor() {
    let error = HttpError::request_timeout("request timed out");
    assert_eq!(error.kind, HttpErrorKind::RequestTimeout);
    assert_eq!(error.message, "request timed out");
}

#[test]
fn test_http_error_retry_layer_constructors_and_retry_hints() {
    let attempt = HttpError::retry_attempt_timeout("attempt budget");
    assert_eq!(attempt.kind, HttpErrorKind::RetryAttemptTimeout);
    assert_eq!(attempt.retry_hint(), RetryHint::NonRetryable);

    let budget = HttpError::retry_max_elapsed_exceeded("max elapsed");
    assert_eq!(budget.kind, HttpErrorKind::RetryMaxElapsedExceeded);
    assert_eq!(budget.retry_hint(), RetryHint::NonRetryable);

    let aborted = HttpError::retry_aborted("policy abort");
    assert_eq!(aborted.kind, HttpErrorKind::RetryAborted);
    assert_eq!(aborted.retry_hint(), RetryHint::NonRetryable);
}

#[test]
fn test_http_error_retry_hint_status_without_status_code_is_non_retryable() {
    let error = HttpError::new(HttpErrorKind::Status, "status missing");
    assert_eq!(error.retry_hint(), RetryHint::NonRetryable);
}

#[test]
fn test_http_error_from_io_error_maps_to_transport_with_source() {
    let io_error = std::io::Error::other("disk gone");
    let error = HttpError::from(io_error);
    assert_eq!(error.kind, HttpErrorKind::Transport);
    assert!(error.message.contains("disk gone"));
    assert!(error.source.is_some());
}

#[test]
fn test_http_error_from_reqwest_error_maps_to_build_client_with_source() {
    let reqwest_error = reqwest::Client::new()
        .get("http://[::1")
        .build()
        .expect_err("invalid URL should fail while building request");
    let error = HttpError::from(reqwest_error);
    assert_eq!(error.kind, HttpErrorKind::BuildClient);
    assert!(error.message.contains("Failed to build reqwest client"));
    assert!(error.source.is_some());
}
