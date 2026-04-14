/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use http::{HeaderMap, Method, StatusCode};
use qubit_http::{
    sse::{DoneMarkerPolicy, SseChunk, SseJsonMode},
    HttpClientFactory, HttpClientOptions, HttpError, HttpErrorKind, HttpStreamResponse,
};
use tokio::time::timeout;

use crate::common::{spawn_one_shot_server, ResponseChunk, ResponsePlan};

#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
struct TestChunk {
    value: i32,
}

fn stream_response_from_chunks(chunks: Vec<Vec<u8>>) -> HttpStreamResponse {
    let stream = futures_util::stream::iter(
        chunks
            .into_iter()
            .map(|bytes| Ok::<Bytes, qubit_http::HttpError>(Bytes::from(bytes))),
    );
    HttpStreamResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        url::Url::parse("https://example.com/stream").unwrap(),
        Box::pin(stream),
    )
}

#[tokio::test]
async fn test_decode_events_reports_sse_protocol_error_on_non_utf8_line() {
    let response = stream_response_from_chunks(vec![vec![0xFF, b'\n']]);
    let mut events = response.decode_events();
    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
}

#[tokio::test]
async fn test_decode_events_handles_chunk_boundaries_and_trailing_flush() {
    let response = stream_response_from_chunks(vec![
        b"data: {\"val".to_vec(),
        b"ue\":1}\n".to_vec(),
        b"\n".to_vec(),
        b"data: {\"value\":2}".to_vec(),
    ]);
    let mut events = response.decode_events();

    let first = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "{\"value\":1}");

    let second = events.next().await.unwrap().unwrap();
    assert_eq!(second.data, "{\"value\":2}");
}

#[tokio::test]
async fn test_decode_events_propagates_upstream_stream_error() {
    let stream = futures_util::stream::iter(vec![Err::<Bytes, HttpError>(HttpError::transport(
        "upstream broken",
    ))]);
    let response = HttpStreamResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        url::Url::parse("https://example.com/stream").unwrap(),
        Box::pin(stream),
    );

    let mut events = response.decode_events();
    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Transport);
}

#[tokio::test]
async fn test_execute_stream_with_decode_events_end_to_end() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![
            ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"data: {\"value\":1}\n\n".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"data: {\"value\":2}\n\n".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client.request(Method::GET, "/sse").build();
    let stream_response = timeout(Duration::from_secs(3), client.execute_stream(request))
        .await
        .expect("execute_stream timed out")
        .unwrap();
    let mut events = stream_response.decode_events();

    let first = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "{\"value\":1}");
    let second = events.next().await.unwrap().unwrap();
    assert_eq!(second.data, "{\"value\":2}");
    assert!(events.next().await.is_none());

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse");
}

#[tokio::test]
async fn test_execute_stream_decode_events_reports_read_timeout_when_interrupted() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![
            ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"data: {\"value\":1}\n\n".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_millis(250),
                bytes: b"data: {\"value\":2}\n\n".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_millis(80);
    options.timeouts.write_timeout = Duration::from_secs(1);
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client.request(Method::GET, "/sse-timeout").build();
    let stream_response = client.execute_stream(request).await.unwrap();
    let mut events = stream_response.decode_events();

    let first = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "{\"value\":1}");

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::ReadTimeout);
}

#[tokio::test]
async fn test_execute_stream_decode_json_chunks_uses_client_default_strict_mode() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![
            ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"data: {\"value\":1}\n\n".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"data: malformed-json\n\n".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.sse_json_mode = SseJsonMode::Strict;
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client.request(Method::GET, "/sse-strict").build();
    let stream_response = client.execute_stream(request).await.unwrap();
    let mut chunks = stream_response.decode_json_chunks::<TestChunk>(DoneMarkerPolicy::DefaultDone);

    let first = chunks.next().await.unwrap().unwrap();
    assert_eq!(first, SseChunk::Data(TestChunk { value: 1 }));

    let error = chunks.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseDecode);
}

#[tokio::test]
async fn test_execute_stream_decode_events_uses_client_default_sse_limits() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![ResponseChunk {
            delay: Duration::from_millis(0),
            bytes: b"data: {\"value\":1}\ndata: {\"value\":2}\n\n".to_vec(),
        }],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.sse_max_frame_bytes = 16;
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client.request(Method::GET, "/sse-limits").build();
    let stream_response = client.execute_stream(request).await.unwrap();
    let mut events = stream_response.decode_events();

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert!(error.message.contains("max_frame_bytes"));
}
