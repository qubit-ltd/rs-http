// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for `src/sse/sse_message.rs`.

use qubit_http::HttpErrorKind;
use qubit_http::sse::SseJsonMode;
use qubit_http::sse::SseMessage;

use crate::common::SensitiveChoice;
use crate::common::capture_trace_logs;

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

    let payload: TestPayload =
        message.decode_json().expect("JSON decoding should succeed");
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
    assert!(
        error
            .message
            .contains("event=Some(\"response.output_text.delta\")")
    );
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

#[test]
fn test_sse_message_decode_json_with_mode_lenient_logs_redacted_diagnostics() {
    const SECRET: &str = "SSE_LOG_SECRET";
    let message = SseMessage {
        event: Some("response.output_text.delta".to_string()),
        data: SECRET.to_string(),
        last_event_id: Some("evt-log".to_string()),
    };

    let logs = capture_trace_logs(|| {
        let payload = message
            .decode_json_with_mode::<TestPayload>(SseJsonMode::Lenient)
            .expect("lenient mode should skip malformed JSON");
        assert!(payload.is_none());
    });

    assert!(logs.contains("error_kind=invalid_json"));
    assert!(logs.contains("error_stage=parse"));
    assert!(logs.contains("event=Some(\"response.output_text.delta\")"));
    assert!(logs.contains("last_event_id=Some(\"evt-log\")"));
    assert!(!logs.contains(SECRET));
}

#[test]
fn test_sse_message_decode_json_with_mode_lenient_normalizes_control_characters()
 {
    let message = SseMessage {
        event: Some("response.output_text.delta".to_string()),
        data: "{\"delta\":\"line\nbreak\"}".to_string(),
        last_event_id: Some("evt-4".to_string()),
    };

    let payload = message
        .decode_json_with_mode::<TestPayload>(SseJsonMode::Lenient)
        .expect("lenient mode should repair supported JSON text");
    assert_eq!(
        payload,
        Some(TestPayload {
            delta: "line\nbreak".to_string(),
        }),
    );
}

#[test]
fn test_sse_message_decode_json_with_mode_distinguishes_control_character_policy()
 {
    let message = SseMessage {
        event: Some("response.output_text.delta".to_string()),
        data: "{\"delta\":\"line\nbreak\"}".to_string(),
        last_event_id: Some("evt-4".to_string()),
    };

    let strict_error = message
        .decode_json_with_mode::<TestPayload>(SseJsonMode::Strict)
        .expect_err("strict SSE JSON must reject raw control characters");
    assert_eq!(strict_error.kind, HttpErrorKind::SseDecode);

    let lenient_payload = message
        .decode_json_with_mode::<TestPayload>(SseJsonMode::Lenient)
        .expect("lenient SSE JSON should repair supported control characters");
    assert_eq!(
        lenient_payload,
        Some(TestPayload {
            delta: "line\nbreak".to_string(),
        }),
    );
}

#[test]
fn test_sse_message_decode_json_redacts_deserializer_value() {
    const SECRET: &str = "SSE_TOP_SECRET";
    let message = SseMessage {
        event: Some("secure.event".to_string()),
        data: format!("\"{SECRET}\""),
        last_event_id: Some("evt-secret".to_string()),
    };
    let error = message
        .decode_json::<SensitiveChoice>()
        .expect_err("unknown enum variant must fail");
    assert!(!error.message.contains(SECRET));
    assert!(error.message.contains("secure.event"));
    assert!(error.message.contains("evt-secret"));
    let source = std::error::Error::source(&error).expect(
        "SSE JSON decode errors must retain the redacted decoder source",
    );
    let decode_error = source
        .downcast_ref::<qubit_json::lenient::LenientJsonDecodeError>()
        .expect("SSE JSON decode source must be JsonDecodeError");
    assert_eq!(
        decode_error.privacy_policy(),
        qubit_json::lenient::ErrorPrivacyPolicy::Redacted,
    );
    assert!(!decode_error.to_string().contains(SECRET));
}
