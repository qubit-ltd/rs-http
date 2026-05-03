/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use bytes::Bytes;
use futures_util::StreamExt;
use http::{
    HeaderMap,
    Method,
    StatusCode,
};
use qubit_http::{
    HttpErrorKind,
    HttpResponse,
};

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
async fn test_decode_events_with_limits_rejects_line_exceeding_max_bytes() {
    let long_line = format!("data: {}\n\n", "a".repeat(64));
    let response = stream_response_from_chunks(vec![long_line]);
    let mut events = response
        .sse_max_line_bytes(16)
        .sse_max_frame_bytes(1024)
        .sse_events();

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert!(error.message.contains("max_line_bytes"));
}

#[tokio::test]
async fn test_decode_events_with_limits_accepts_line_within_max_bytes() {
    let response = stream_response_from_chunks(vec!["data: ok\n\n".to_string()]);
    let mut events = response
        .sse_max_line_bytes(64)
        .sse_max_frame_bytes(1024)
        .sse_events();

    let event = events.next().await.unwrap().unwrap();
    assert_eq!(event.data, "ok");
    assert!(events.next().await.is_none());
}
