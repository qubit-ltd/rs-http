/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Integration tests for `src/sse/mod.rs` (`decode_events`).
//! File layout: `tests/sse/mod_tests.rs` mirrors `src/sse/mod.rs`.

use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt as _;
use http::{HeaderMap, Method};
use qubit_http::{sse::SseReconnectOptions, HttpResponse, HttpResult, RetryDelay, RetryJitter};

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
async fn test_decode_events_parses_fields_and_multiline_data() {
    let response = stream_response_from_chunks(vec![
        "event: message\r\nid: evt-1\r\ndata: line-1\r\ndata: line-2\r\nretry: 123\r\n\r\n",
    ]);
    let events = collect_results(response.sse_events()).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event.as_deref(), Some("message"));
    assert_eq!(events[0].id.as_deref(), Some("evt-1"));
    assert_eq!(events[0].retry, Some(123));
    assert_eq!(events[0].data, "line-1\nline-2");
}

#[tokio::test]
async fn test_decode_events_ignores_comment_lines() {
    let response =
        stream_response_from_chunks(vec![": keep-alive\n", "data: {\"value\": 7}\n", "\n"]);
    let events = collect_results(response.sse_events()).await;
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
    assert_eq!(options.retry.max_attempts(), 4);
    assert_eq!(
        options.retry.delay(),
        &RetryDelay::exponential(Duration::from_secs(1), Duration::from_secs(30), 2.0,)
    );
    assert_eq!(options.retry.jitter(), RetryJitter::None);
}
