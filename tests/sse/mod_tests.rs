// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Integration tests for `src/sse/mod.rs` (`decode_messages`).
//! File layout: `tests/sse/mod_tests.rs` mirrors `src/sse/mod.rs`.

use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt as _;
use http::HeaderMap;
use http::Method;
use qubit_http::sse::SseReconnectOptions;
use qubit_http::HttpResponse;
use qubit_http::HttpResult;

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
async fn test_decode_messages_parses_fields_and_multiline_data() {
    let response = stream_response_from_chunks(vec![
        "event: message\r\nid: evt-1\r\ndata: line-1\r\ndata: line-2\r\nretry: 123\r\n\r\n",
    ]);
    let events = collect_results(response.sse_messages()).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event.as_deref(), Some("message"));
    assert_eq!(events[0].last_event_id.as_deref(), Some("evt-1"));
    assert_eq!(events[0].data, "line-1\nline-2");
}

#[tokio::test]
async fn test_decode_messages_ignores_comment_lines() {
    let response =
        stream_response_from_chunks(vec![": keep-alive\n", "data: {\"value\": 7}\n", "\n"]);
    let events = collect_results(response.sse_messages()).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "{\"value\": 7}");
}

#[test]
fn test_sse_reconnect_options_new_matches_default() {
    assert_eq!(SseReconnectOptions::new(), SseReconnectOptions::default());
}

#[test]
fn test_sse_reconnect_options_default_backoff_parameters() {
    let options = SseReconnectOptions::default();
    assert_eq!(options.retry.limits().max_attempts().get(), 4);
    assert_eq!(
        options.retry.backoff().maximum_delay(),
        Some(Duration::from_secs(30)),
    );
}

#[test]
fn test_sse_reconnect_options_default_server_retry_controls() {
    let options = SseReconnectOptions::default();
    assert!(options.honor_server_retry);
    assert_eq!(options.server_retry_max_delay, None);
    assert!(options.apply_jitter_to_server_retry);
}

#[test]
fn test_sse_reconnect_options_can_override_server_retry_controls() {
    let options = SseReconnectOptions {
        honor_server_retry: false,
        server_retry_max_delay: Some(Duration::from_millis(250)),
        apply_jitter_to_server_retry: false,
        ..SseReconnectOptions::default()
    };
    assert!(!options.honor_server_retry);
    assert_eq!(
        options.server_retry_max_delay,
        Some(Duration::from_millis(250))
    );
    assert!(!options.apply_jitter_to_server_retry);
}
