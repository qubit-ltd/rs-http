// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use bytes::Bytes;
use futures_util::Stream;
use futures_util::StreamExt;
use http::HeaderMap;
use http::Method;
use http::StatusCode;
use qubit_http::HttpErrorKind;
use qubit_http::HttpResponse;
use qubit_http::HttpResult;
use tokio::test as tokio_test;
use url::Url;

async fn collect_results<T>(stream: impl Stream<Item = HttpResult<T>>) -> Vec<T> {
    stream
        .map(|item| item.expect("unexpected stream error in test"))
        .collect::<Vec<_>>()
        .await
}

fn stream_response_from_chunks(chunks: Vec<&'static str>) -> HttpResponse {
    let body = chunks.join("");
    HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from(body),
        Url::parse("https://example.com/stream").unwrap(),
        Method::GET,
    )
}

#[tokio_test]
async fn test_decode_messages_does_not_emit_retry_only_control_frame() {
    let response = stream_response_from_chunks(vec!["retry: 100\n", "\n", "data: hello\n", "\n"]);
    let messages = collect_results(response.sse_messages()).await;

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].data, "hello");
}

#[tokio_test]
async fn test_decode_messages_applies_control_only_id_to_next_message() {
    let response = stream_response_from_chunks(vec!["id: resume-token\n", "\n", "data: hello\n", "\n"]);
    let messages = collect_results(response.sse_messages()).await;

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].last_event_id.as_deref(), Some("resume-token"));
}

#[tokio_test]
async fn test_decode_frames_allows_field_without_colon_as_field_name() {
    let response = stream_response_from_chunks(vec!["data\n", "\n"]);
    let events = collect_results(response.sse_messages()).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "");
    assert_eq!(events[0].event, None);
}

#[tokio_test]
async fn test_decode_frames_preserves_empty_data_lines_and_embedded_newlines() {
    let response = stream_response_from_chunks(vec!["data\n", "\n"]);
    let events = collect_results(response.sse_messages()).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "");

    let response = stream_response_from_chunks(vec!["data:\n", "data:\n", "\n"]);
    let events = collect_results(response.sse_messages()).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "\n");

    let response = stream_response_from_chunks(vec!["data: a\n", "data:\n", "data: b\n\n"]);
    let events = collect_results(response.sse_messages()).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "a\n\nb");
}

#[tokio_test]
async fn test_decode_frames_accepts_near_limit_short_lines_then_resets_for_next_frame() {
    let mut body = String::new();
    for index in 0..63 {
        body.push_str(if index == 0 { "data: x\n" } else { "data:\n" });
    }
    body.push_str("\ndata: z\n\n");
    let response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from(body),
        Url::parse("https://example.com/stream").unwrap(),
        Method::GET,
    );
    let frame_bytes = 7 + 62 * 5;
    let events = collect_results(
        response
            .sse_max_line_bytes(128)
            .sse_max_frame_bytes(frame_bytes)
            .sse_messages(),
    )
    .await;

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].data, format!("x{}", "\n".repeat(62)));
    assert_eq!(events[1].data, "z");

    let response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from(format!("{}data: z\n\n", "data: x\n".repeat(63))),
        Url::parse("https://example.com/stream").unwrap(),
        Method::GET,
    );
    let mut events = response
        .sse_max_line_bytes(128)
        .sse_max_frame_bytes(frame_bytes)
        .sse_messages();
    assert_eq!(
        events.next().await.unwrap().unwrap_err().kind,
        HttpErrorKind::SseProtocol,
    );
}

#[tokio_test]
async fn test_decode_frames_handles_invalid_retry_value_as_known_field() {
    let response = stream_response_from_chunks(vec!["data: hi\n", "retry: bad\n", "\n"]);
    let events = collect_results(response.sse_messages()).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hi");
}

#[tokio_test]
async fn test_decode_frames_ignores_invalid_retry_values_without_protocol_error() {
    for retry in ["+120", "-1", "", " 120", "١٢٠", "18446744073709551616"] {
        let response = stream_response_from_chunks(vec!["retry: ", retry, "\ndata: hi\n\n"]);
        let events = collect_results(response.sse_messages()).await;

        assert_eq!(events.len(), 1, "retry value {retry:?} must be ignored");
        assert_eq!(events[0].data, "hi");
    }
}

#[tokio_test]
async fn test_decode_frames_ignores_unknown_field_name() {
    let response = stream_response_from_chunks(vec!["unknown: ignored\n", "data: value\n", "\n"]);
    let events = collect_results(response.sse_messages()).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "value");
}

#[tokio_test]
async fn test_decode_frames_rejects_frame_exceeding_max_bytes() {
    let response = stream_response_from_chunks(vec!["data: 12345\n", "data: 67890\n", "\n"]);
    let mut events = response.sse_max_line_bytes(128).sse_max_frame_bytes(12).sse_messages();
    let error = events.next().await.unwrap().unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert!(error.message.contains("max_frame_bytes"));
}

#[tokio_test]
async fn test_decode_frames_accepts_frame_at_max_bytes() {
    let response = stream_response_from_chunks(vec!["data: value\n", "\n"]);
    let mut events = response
        .sse_max_line_bytes(128)
        .sse_max_frame_bytes("data: value".len())
        .sse_messages();

    let event = events
        .next()
        .await
        .expect("SSE stream should yield the event")
        .expect("frame at configured limit should be accepted");

    assert_eq!(event.data, "value");
    assert!(events.next().await.is_none());
}

#[tokio_test]
async fn test_decode_frames_ignores_comment_lines() {
    let response = stream_response_from_chunks(vec![": heartbeat\n", "data: hello\n", "\n"]);
    let events = collect_results(response.sse_max_line_bytes(128).sse_max_frame_bytes(64).sse_messages()).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hello");
}

#[tokio_test]
async fn test_decode_frames_discards_unterminated_event_at_eof() {
    for body in ["data: value", "data: value\n", "data: value\r", "data: value\r\n"] {
        let response = stream_response_from_chunks(vec![body]);
        let events = collect_results(response.sse_messages()).await;
        assert!(events.is_empty(), "unterminated frame {body:?} must not dispatch");
    }
    for body in ["data: value\r\r", "data: value\n\n", "data: value\r\n\r\n"] {
        let response = stream_response_from_chunks(vec![body]);
        let events = collect_results(response.sse_messages()).await;
        assert_eq!(events.len(), 1, "complete frame {body:?} should dispatch");
        assert_eq!(events[0].data, "value");
    }

    let response = stream_response_from_chunks(vec!["id: pending\ndata: incomplete"]);
    assert!(collect_results(response.sse_messages()).await.is_empty());
}

#[tokio_test]
async fn test_decode_frames_accepts_field_value_without_space_after_colon() {
    let response = stream_response_from_chunks(vec!["event:update\n", "data:value\n", "\n"]);
    let events = collect_results(response.sse_messages()).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event.as_deref(), Some("update"));
    assert_eq!(events[0].data, "value");
}

#[tokio_test]
async fn test_decode_frames_resets_budget_after_unknown_only_frame() {
    let response = stream_response_from_chunks(vec!["x: 123\n\n", "x: 123\n\n", "data: a\n\n"]);
    let mut events = response.sse_max_line_bytes(128).sse_max_frame_bytes(8).sse_messages();
    let event = events
        .next()
        .await
        .expect("message should be emitted")
        .expect("each empty line resets frame budget");
    assert_eq!(event.data, "a");
    assert!(events.next().await.is_none());
}
