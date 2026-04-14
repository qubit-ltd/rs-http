/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for `src/sse/sse_event.rs`.

use qubit_http::sse::SseEvent;
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
