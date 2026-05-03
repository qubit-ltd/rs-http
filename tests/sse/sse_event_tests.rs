/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for `src/sse/sse_event.rs`.

use qubit_http::sse::{
    SseEvent,
    SseJsonMode,
};
use qubit_http::HttpErrorKind;

#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
struct TestPayload {
    delta: String,
}

#[test]
fn test_sse_event_decode_json_success() {
    let event = SseEvent {
        event: Some("response.output_text.delta".to_string()),
        data: r#"{"delta":"hello"}"#.to_string(),
        id: Some("evt-1".to_string()),
        retry: None,
    };

    let payload: TestPayload = event.decode_json().expect("JSON decoding should succeed");
    assert_eq!(
        payload,
        TestPayload {
            delta: "hello".to_string(),
        }
    );
}

#[test]
fn test_sse_event_decode_json_error_is_sse_decode_with_context() {
    let event = SseEvent {
        event: Some("response.output_text.delta".to_string()),
        data: "not-json".to_string(),
        id: Some("evt-2".to_string()),
        retry: None,
    };

    let error = event
        .decode_json::<TestPayload>()
        .expect_err("invalid JSON should fail");
    assert_eq!(error.kind, HttpErrorKind::SseDecode);
    assert!(error
        .message
        .contains("event=Some(\"response.output_text.delta\")"));
    assert!(error.message.contains("id=Some(\"evt-2\")"));
}

#[test]
fn test_sse_event_decode_json_with_mode_lenient_returns_none_for_bad_json() {
    let event = SseEvent {
        event: Some("response.output_text.delta".to_string()),
        data: "not-json".to_string(),
        id: Some("evt-3".to_string()),
        retry: None,
    };

    let payload = event
        .decode_json_with_mode::<TestPayload>(SseJsonMode::Lenient)
        .expect("lenient mode should not fail");
    assert!(payload.is_none());
}

#[test]
fn test_sse_event_decode_json_with_mode_lenient_returns_some_for_valid_json() {
    let event = SseEvent {
        event: Some("response.output_text.delta".to_string()),
        data: r#"{"delta":"ok"}"#.to_string(),
        id: Some("evt-lenient-ok".to_string()),
        retry: None,
    };

    let payload = event
        .decode_json_with_mode::<TestPayload>(SseJsonMode::Lenient)
        .expect("lenient mode should decode valid JSON");
    assert_eq!(
        payload,
        Some(TestPayload {
            delta: "ok".to_string(),
        })
    );
}

#[test]
fn test_sse_event_decode_json_with_mode_strict_fails_for_bad_json() {
    let event = SseEvent {
        event: Some("response.output_text.delta".to_string()),
        data: "not-json".to_string(),
        id: Some("evt-4".to_string()),
        retry: None,
    };

    let error = event
        .decode_json_with_mode::<TestPayload>(SseJsonMode::Strict)
        .expect_err("strict mode should fail on invalid JSON");
    assert_eq!(error.kind, HttpErrorKind::SseDecode);
}
