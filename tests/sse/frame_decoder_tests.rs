// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use bytes::Bytes;
use futures_util::StreamExt;
use http::HeaderMap;
use http::Method;
use qubit_http::HttpResponse;
use qubit_http::HttpResult;

async fn collect_results<T>(
    stream: impl futures_util::Stream<Item = HttpResult<T>>,
) -> Vec<T> {
    stream
        .map(|item| item.expect("unexpected stream error in test"))
        .collect::<Vec<_>>()
        .await
}

fn stream_response_from_chunks(chunks: Vec<&'static str>) -> HttpResponse {
    let body = chunks.join("");
    HttpResponse::new(
        http::StatusCode::OK,
        HeaderMap::new(),
        Bytes::from(body),
        url::Url::parse("https://example.com/stream").unwrap(),
        Method::GET,
    )
}

#[tokio::test]
async fn test_decode_messages_does_not_emit_retry_only_control_frame() {
    let response = stream_response_from_chunks(vec![
        "retry: 100\n",
        "\n",
        "data: hello\n",
        "\n",
    ]);
    let messages = collect_results(response.sse_messages()).await;

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].data, "hello");
}

#[tokio::test]
async fn test_decode_messages_applies_control_only_id_to_next_message() {
    let response = stream_response_from_chunks(vec![
        "id: resume-token\n",
        "\n",
        "data: hello\n",
        "\n",
    ]);
    let messages = collect_results(response.sse_messages()).await;

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].last_event_id.as_deref(), Some("resume-token"));
}

#[tokio::test]
async fn test_decode_frames_allows_field_without_colon_as_field_name() {
    let response = stream_response_from_chunks(vec!["data\n", "\n"]);
    let events = collect_results(response.sse_messages()).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "");
    assert_eq!(events[0].event, None);
}

#[tokio::test]
async fn test_decode_frames_handles_invalid_retry_value_as_known_field() {
    let response =
        stream_response_from_chunks(vec!["data: hi\n", "retry: bad\n", "\n"]);
    let events = collect_results(response.sse_messages()).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hi");
}

#[tokio::test]
async fn test_decode_frames_ignores_unknown_field_name() {
    let response = stream_response_from_chunks(vec![
        "unknown: ignored\n",
        "data: value\n",
        "\n",
    ]);
    let events = collect_results(response.sse_messages()).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "value");
}

#[tokio::test]
async fn test_decode_frames_rejects_frame_exceeding_max_bytes() {
    let response = stream_response_from_chunks(vec![
        "data: 12345\n",
        "data: 67890\n",
        "\n",
    ]);
    let mut events = response
        .sse_max_line_bytes(128)
        .sse_max_frame_bytes(12)
        .sse_messages();
    let error = events.next().await.unwrap().unwrap_err();

    assert_eq!(error.kind, qubit_http::HttpErrorKind::SseProtocol);
    assert!(error.message.contains("max_frame_bytes"));
}

#[tokio::test]
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

#[tokio::test]
async fn test_decode_frames_ignores_comment_lines() {
    let response = stream_response_from_chunks(vec![
        ": heartbeat\n",
        "data: hello\n",
        "\n",
    ]);
    let events = collect_results(
        response
            .sse_max_line_bytes(128)
            .sse_max_frame_bytes(64)
            .sse_messages(),
    )
    .await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hello");
}

#[tokio::test]
async fn test_decode_frames_emits_last_event_without_trailing_blank_line() {
    let response = stream_response_from_chunks(vec!["data: final"]);
    let events = collect_results(response.sse_messages()).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "final");
}

#[tokio::test]
async fn test_decode_frames_accepts_field_value_without_space_after_colon() {
    let response = stream_response_from_chunks(vec![
        "event:update\n",
        "data:value\n",
        "\n",
    ]);
    let events = collect_results(response.sse_messages()).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event.as_deref(), Some("update"));
    assert_eq!(events[0].data, "value");
}
