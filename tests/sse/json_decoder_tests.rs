/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Tests for `src/sse/json_decoder.rs`.

use bytes::Bytes;
use futures_util::StreamExt;
use http::{
    HeaderMap,
    Method,
};
use qubit_http::sse::{
    DoneMarkerPolicy,
    SseChunk,
    SseJsonMode,
};
use qubit_http::{
    HttpResponse,
    HttpResult,
};

#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
struct TestChunk {
    value: i32,
}

async fn collect_results<T>(stream: impl futures_util::Stream<Item = HttpResult<T>>) -> Vec<T> {
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
async fn test_decode_json_chunks_lenient_skips_bad_json_and_respects_done() {
    let response = stream_response_from_chunks(vec![
        "data: {\"value\": 1}\n\n",
        "data: malformed-json\n\n",
        "data: [DONE]\n\n",
        "data: {\"value\": 9}\n\n",
    ]);
    let chunks = collect_results(response.sse_chunks::<TestChunk>()).await;

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0], SseChunk::Data(TestChunk { value: 1 }));
    assert_eq!(chunks[1], SseChunk::Done);
}

#[tokio::test]
async fn test_decode_json_chunks_strict_fails_on_bad_json() {
    let response =
        stream_response_from_chunks(vec!["data: {\"value\": 1}\n\n", "data: malformed-json\n\n"]);
    let mut stream = response
        .sse_json_mode(SseJsonMode::Strict)
        .sse_chunks::<TestChunk>();

    let first = stream.next().await.unwrap().unwrap();
    assert_eq!(first, SseChunk::Data(TestChunk { value: 1 }));

    let second = stream.next().await.unwrap();
    let error = second.unwrap_err();
    assert_eq!(error.kind, qubit_http::HttpErrorKind::SseDecode);
}

#[tokio::test]
async fn test_decode_json_chunks_strict_error_includes_message_context() {
    let response = stream_response_from_chunks(vec![
        "event: response.delta\n",
        "id: evt-42\n",
        "data: malformed-json\n\n",
    ]);
    let mut stream = response
        .sse_json_mode(SseJsonMode::Strict)
        .sse_chunks::<TestChunk>();

    let error = stream.next().await.unwrap().unwrap_err();

    assert_eq!(error.kind, qubit_http::HttpErrorKind::SseDecode);
    assert!(error.message.contains("event=Some(\"response.delta\")"));
    assert!(error.message.contains("last_event_id=Some(\"evt-42\")"));
}

#[tokio::test]
async fn test_decode_json_chunks_with_custom_done_marker() {
    let response = stream_response_from_chunks(vec!["data: {\"value\": 2}\n\n", "data: <END>\n\n"]);
    let chunks = collect_results(
        response
            .sse_done_marker_policy(DoneMarkerPolicy::Custom("<END>".to_string()))
            .sse_chunks::<TestChunk>(),
    )
    .await;

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0], SseChunk::Data(TestChunk { value: 2 }));
    assert_eq!(chunks[1], SseChunk::Done);
}

#[tokio::test]
async fn test_decode_json_chunks_with_limits_reports_sse_protocol_error() {
    let response = stream_response_from_chunks(vec![
        "data: {\"value\": 1}\n",
        "data: {\"value\": 2}\n",
        "\n",
    ]);
    let mut stream = response
        .sse_json_mode(SseJsonMode::Strict)
        .sse_max_line_bytes(256)
        .sse_max_frame_bytes(16)
        .sse_chunks::<TestChunk>();

    let error = stream.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, qubit_http::HttpErrorKind::SseProtocol);
    assert!(error.message.contains("max_frame_bytes"));
}

/// Regression: `sse_json_mode` → `sse_done_marker_policy` → `sse_max_line_bytes` →
/// `sse_max_frame_bytes` → `sse_chunks()` must compile and decode using that chain (see user guide
/// “Configure `sse_chunks` options”).
#[tokio::test]
async fn test_regression_sse_chunks_chain_setters_before_decode() {
    let response = stream_response_from_chunks(vec!["data: {\"value\": 7}\n\n"]);
    let chunks = collect_results(
        response
            .sse_json_mode(SseJsonMode::Strict)
            .sse_done_marker_policy(DoneMarkerPolicy::DefaultDone)
            .sse_max_line_bytes(256)
            .sse_max_frame_bytes(16 * 1024)
            .sse_chunks::<TestChunk>(),
    )
    .await;
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], SseChunk::Data(TestChunk { value: 7 }));
}
