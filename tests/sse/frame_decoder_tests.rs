/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use bytes::Bytes;
use futures_util::StreamExt;
use http::HeaderMap;
use qubit_http::sse::decode_events;
use qubit_http::{HttpResult, HttpStreamResponse};

async fn collect_results<T>(stream: impl futures_util::Stream<Item = HttpResult<T>>) -> Vec<T> {
    stream
        .map(|item| item.expect("unexpected stream error in test"))
        .collect::<Vec<_>>()
        .await
}

fn stream_response_from_chunks(chunks: Vec<&'static str>) -> HttpStreamResponse {
    let stream = futures_util::stream::iter(
        chunks
            .into_iter()
            .map(|text| Ok::<Bytes, qubit_http::HttpError>(Bytes::from(text.to_string()))),
    );
    HttpStreamResponse::new(
        http::StatusCode::OK,
        HeaderMap::new(),
        url::Url::parse("https://example.com/stream").unwrap(),
        Box::pin(stream),
    )
}

#[tokio::test]
async fn test_decode_frames_allows_field_without_colon_as_field_name() {
    let response = stream_response_from_chunks(vec!["data\n", "\n"]);
    let events = collect_results(decode_events(response)).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "");
    assert_eq!(events[0].event, None);
}

#[tokio::test]
async fn test_decode_frames_handles_invalid_retry_value_as_known_field() {
    let response = stream_response_from_chunks(vec!["data: hi\n", "retry: bad\n", "\n"]);
    let events = collect_results(decode_events(response)).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "hi");
    assert_eq!(events[0].retry, None);
}

#[tokio::test]
async fn test_decode_frames_ignores_unknown_field_name() {
    let response = stream_response_from_chunks(vec!["unknown: ignored\n", "data: value\n", "\n"]);
    let events = collect_results(decode_events(response)).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "value");
}
