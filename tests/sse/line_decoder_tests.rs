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
use http::{HeaderMap, StatusCode};
use qubit_http::{HttpErrorKind, HttpStreamResponse};

fn stream_response_from_chunks(chunks: Vec<String>) -> HttpStreamResponse {
    let stream = futures_util::stream::iter(
        chunks
            .into_iter()
            .map(|text| Ok::<Bytes, qubit_http::HttpError>(Bytes::from(text))),
    );
    HttpStreamResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        url::Url::parse("https://example.com/stream").unwrap(),
        Box::pin(stream),
    )
}

#[tokio::test]
async fn test_decode_events_with_limits_rejects_line_exceeding_max_bytes() {
    let long_line = format!("data: {}\n\n", "a".repeat(64));
    let response = stream_response_from_chunks(vec![long_line]);
    let mut events = response.decode_events_with_limits(16, 1024);

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert!(error.message.contains("max_line_bytes"));
}

#[tokio::test]
async fn test_decode_events_with_limits_accepts_line_within_max_bytes() {
    let response = stream_response_from_chunks(vec!["data: ok\n\n".to_string()]);
    let mut events = response.decode_events_with_limits(64, 1024);

    let event = events.next().await.unwrap().unwrap();
    assert_eq!(event.data, "ok");
    assert!(events.next().await.is_none());
}
