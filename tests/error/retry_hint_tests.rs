// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Integration tests for `src/error/retry_hint.rs`.

use http::StatusCode;
use qubit_http::{
    HttpError,
    RetryHint,
};

use std::str::FromStr;

#[test]
fn test_retry_hint_retryable_for_timeout_and_transport_errors() {
    let connect_timeout = HttpError::connect_timeout("connect timeout");
    let request_timeout = HttpError::request_timeout("request timeout");
    let read_timeout = HttpError::read_timeout("read timeout");
    let write_timeout = HttpError::write_timeout("write timeout");
    let transport = HttpError::transport("transport error");

    assert_eq!(connect_timeout.retry_hint(), RetryHint::Retryable);
    assert_eq!(request_timeout.retry_hint(), RetryHint::Retryable);
    assert_eq!(read_timeout.retry_hint(), RetryHint::Retryable);
    assert_eq!(write_timeout.retry_hint(), RetryHint::Retryable);
    assert_eq!(transport.retry_hint(), RetryHint::Retryable);
}

#[test]
fn test_retry_hint_for_status_code() {
    let too_many_requests =
        HttpError::status(StatusCode::TOO_MANY_REQUESTS, "429");
    let internal_error =
        HttpError::status(StatusCode::INTERNAL_SERVER_ERROR, "500");
    let bad_request = HttpError::status(StatusCode::BAD_REQUEST, "400");

    assert_eq!(too_many_requests.retry_hint(), RetryHint::Retryable);
    assert_eq!(internal_error.retry_hint(), RetryHint::Retryable);
    assert_eq!(bad_request.retry_hint(), RetryHint::NonRetryable);
}

#[test]
fn test_retry_hint_non_retryable_for_protocol_and_config_errors() {
    let invalid_url = HttpError::invalid_url("invalid url");
    let proxy_config = HttpError::proxy_config("proxy config");
    let sse_protocol = HttpError::sse_protocol("sse protocol");
    let sse_decode = HttpError::sse_decode("sse decode");
    let retry_attempt_timeout =
        HttpError::retry_attempt_timeout("attempt timeout");
    let retry_max_elapsed =
        HttpError::retry_max_elapsed_exceeded("max elapsed");
    let retry_aborted = HttpError::retry_aborted("aborted");
    let other = HttpError::other("other");

    assert_eq!(invalid_url.retry_hint(), RetryHint::NonRetryable);
    assert_eq!(proxy_config.retry_hint(), RetryHint::NonRetryable);
    assert_eq!(sse_protocol.retry_hint(), RetryHint::NonRetryable);
    assert_eq!(sse_decode.retry_hint(), RetryHint::NonRetryable);
    assert_eq!(retry_attempt_timeout.retry_hint(), RetryHint::NonRetryable);
    assert_eq!(retry_max_elapsed.retry_hint(), RetryHint::NonRetryable);
    assert_eq!(retry_aborted.retry_hint(), RetryHint::NonRetryable);
    assert_eq!(other.retry_hint(), RetryHint::NonRetryable);
}

#[test]
fn test_retry_hint_from_str_and_roundtrip() {
    assert_eq!(
        RetryHint::from_str("retryable").expect("retryable"),
        RetryHint::Retryable
    );
    assert_eq!(
        RetryHint::from_str("non_retryable").expect("non retryable"),
        RetryHint::NonRetryable
    );
    assert_eq!(RetryHint::Retryable.to_string(), "retryable");
}
