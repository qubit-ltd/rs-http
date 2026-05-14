/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for `src/sse/sse_message.rs`.

use qubit_http::sse::{
    SseJsonMode,
    SseMessage,
};
use qubit_http::HttpErrorKind;

#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
struct TestPayload {
    delta: String,
}

#[test]
fn test_sse_message_decode_json_success() {
    let message = SseMessage {
        event: Some("response.output_text.delta".to_string()),
        data: r#"{"delta":"hello"}"#.to_string(),
        last_event_id: Some("evt-1".to_string()),
    };

    let payload: TestPayload = message.decode_json().expect("JSON decoding should succeed");
    assert_eq!(
        payload,
        TestPayload {
            delta: "hello".to_string(),
        }
    );
}

#[test]
fn test_sse_message_decode_json_error_is_sse_decode_with_context() {
    let message = SseMessage {
        event: Some("response.output_text.delta".to_string()),
        data: "not-json".to_string(),
        last_event_id: Some("evt-2".to_string()),
    };

    let error = message
        .decode_json::<TestPayload>()
        .expect_err("invalid JSON should fail");
    assert_eq!(error.kind, HttpErrorKind::SseDecode);
    assert!(error
        .message
        .contains("event=Some(\"response.output_text.delta\")"));
    assert!(error.message.contains("last_event_id=Some(\"evt-2\")"));
}

#[test]
fn test_sse_message_decode_json_with_mode_lenient_returns_none_for_bad_json() {
    let message = SseMessage {
        event: Some("response.output_text.delta".to_string()),
        data: "not-json".to_string(),
        last_event_id: Some("evt-3".to_string()),
    };

    let payload = message
        .decode_json_with_mode::<TestPayload>(SseJsonMode::Lenient)
        .expect("lenient mode should not fail");
    assert!(payload.is_none());
}
