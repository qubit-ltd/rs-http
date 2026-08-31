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
use http::StatusCode;
use qubit_http::HttpErrorKind;
use qubit_http::HttpResponse;

fn stream_response_from_chunks(chunks: Vec<String>) -> HttpResponse {
    let body = chunks.join("");
    HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from(body),
        url::Url::parse("https://example.com/stream").unwrap(),
        Method::GET,
    )
}

#[tokio::test]
async fn test_decode_events_accepts_cr_only_line_endings() {
    let response = stream_response_from_chunks(vec!["data: one\r\rdata: two\r\r".to_string()]);
    let mut events = response.sse_max_line_bytes(64).sse_max_frame_bytes(1024).sse_messages();

    let first = events.next().await.unwrap().unwrap();
    let second = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "one");
    assert_eq!(second.data, "two");
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn test_decode_events_accepts_crlf_split_across_chunks() {
    let response = stream_response_from_chunks(vec![
        "data: one\r".to_string(),
        "\r".to_string(),
        "\ndata: two\r".to_string(),
        "\n\r".to_string(),
        "\n".to_string(),
    ]);
    let mut events = response.sse_max_line_bytes(64).sse_max_frame_bytes(1024).sse_messages();

    let first = events.next().await.unwrap().unwrap();
    let second = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "one");
    assert_eq!(second.data, "two");
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn test_decode_events_accepts_dense_mixed_line_endings_in_one_chunk() {
    let response =
        stream_response_from_chunks(vec!["event: add\ndata: one\n\revent: add\rdata: two\r\r\n".to_string()]);
    let mut events = response.sse_max_line_bytes(64).sse_max_frame_bytes(1024).sse_messages();

    let first = events.next().await.unwrap().unwrap();
    let second = events.next().await.unwrap().unwrap();

    assert_eq!(first.event.as_deref(), Some("add"));
    assert_eq!(first.data, "one");
    assert_eq!(second.event.as_deref(), Some("add"));
    assert_eq!(second.data, "two");
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn test_decode_events_with_limits_rejects_line_exceeding_max_bytes() {
    let long_line = format!("data: {}\n\n", "a".repeat(64));
    let response = stream_response_from_chunks(vec![long_line]);
    let mut events = response.sse_max_line_bytes(16).sse_max_frame_bytes(1024).sse_messages();

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert!(error.message.contains("max_line_bytes"));
}

#[tokio::test]
async fn test_decode_events_rejects_invalid_utf8_line() {
    let response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"data: \xFF\n\n"),
        url::Url::parse("https://example.com/stream").expect("valid URL"),
        Method::GET,
    );
    let mut events = response.sse_max_line_bytes(64).sse_max_frame_bytes(1024).sse_messages();

    let error = events.next().await.unwrap().unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert!(error.message.contains("UTF-8"));
}

#[tokio::test]
async fn test_decode_events_with_limits_accepts_line_within_max_bytes() {
    let response = stream_response_from_chunks(vec!["data: ok\n\n".to_string()]);
    let mut events = response.sse_max_line_bytes(64).sse_max_frame_bytes(1024).sse_messages();

    let event = events.next().await.unwrap().unwrap();
    assert_eq!(event.data, "ok");
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn test_decode_events_with_limits_accepts_line_at_max_bytes() {
    let response = stream_response_from_chunks(vec!["data: ok\n\n".to_string()]);
    let mut events = response
        .sse_max_line_bytes("data: ok".len())
        .sse_max_frame_bytes(1024)
        .sse_messages();

    let event = events
        .next()
        .await
        .expect("SSE stream should yield the event")
        .expect("line at configured limit should be accepted");

    assert_eq!(event.data, "ok");
    assert!(events.next().await.is_none());
}
