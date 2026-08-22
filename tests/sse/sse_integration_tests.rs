// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use http::HeaderMap;
use http::Method;
use http::StatusCode;
use qubit_http::HttpClientFactory;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use qubit_http::HttpResponse;
use qubit_http::sse::SseChunk;
use qubit_http::sse::SseJsonMode;
use qubit_redact::RedactionPolicy;
use qubit_redact::Sensitivity;
use tokio::time::timeout;

use crate::common::ResponseChunk;
use crate::common::ResponsePlan;
use crate::common::spawn_one_shot_server;

#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
struct TestChunk {
    value: i32,
}

fn stream_response_from_chunks(chunks: Vec<Vec<u8>>) -> HttpResponse {
    let body = chunks.into_iter().flatten().collect::<Vec<u8>>();
    HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from(body),
        url::Url::parse("https://example.com/stream").unwrap(),
        Method::GET,
    )
}

#[tokio::test]
async fn test_decode_events_reports_sse_protocol_error_on_non_utf8_line() {
    let response = stream_response_from_chunks(vec![vec![0xFF, b'\n']]);
    let mut events = response.sse_messages();
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
    let mut events = response.sse_messages();

    let first = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "{\"value\":1}");

    let second = events.next().await.unwrap().unwrap();
    assert_eq!(second.data, "{\"value\":2}");
}

#[tokio::test]
async fn test_decode_events_reports_frame_limit_error() {
    let response =
        stream_response_from_chunks(vec![b"data: one\ndata: two\n\n".to_vec()]);
    let mut events = response
        .sse_max_line_bytes(1024)
        .sse_max_frame_bytes(8)
        .sse_messages();
    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
}

/// Regression: `sse_max_line_bytes` → `sse_max_frame_bytes` → `sse_messages()`
/// must compile and apply limits from the same chain (see user guide “Configure
/// `sse_messages` options”).
#[tokio::test]
async fn test_regression_sse_messages_chain_setters_before_decode() {
    let response = stream_response_from_chunks(vec![b"data: ok\n\n".to_vec()]);
    let mut events = response
        .sse_max_line_bytes(64 * 1024)
        .sse_max_frame_bytes(1024 * 1024)
        .sse_messages();
    let ev = events.next().await.unwrap().unwrap();
    assert_eq!(ev.data, "ok");
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn test_execute_stream_with_decode_events_end_to_end() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![(
            "Content-Type".to_string(),
            "text/event-stream".to_string(),
        )],
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
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client.request(Method::GET, "/sse").build();
    let stream_response =
        timeout(Duration::from_secs(3), client.execute(request))
            .await
            .expect("execute timed out")
            .unwrap();
    let mut events = stream_response.sse_messages();

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
async fn test_execute_stream_decode_events_reports_read_timeout_when_interrupted()
 {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![(
            "Content-Type".to_string(),
            "text/event-stream".to_string(),
        )],
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
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client.request(Method::GET, "/sse-timeout").build();
    let stream_response = client.execute(request).await.unwrap();
    let mut events = stream_response.sse_messages();

    let first = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "{\"value\":1}");

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::ReadTimeout);
}

#[tokio::test]
async fn test_execute_stream_decode_json_chunks_uses_client_default_strict_mode()
 {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![(
            "Content-Type".to_string(),
            "text/event-stream".to_string(),
        )],
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
    let builder = RedactionPolicy::default()
        .to_builder()
        .http(|http| {
            let _ = http.body().raise("sse_decode_secret", Sensitivity::Secret);
        })
        .expect("the test policy input should be valid");
    let expected_policy = builder
        .build()
        .expect("the custom HTTP policy should be valid");
    options.log_redaction_policy = expected_policy.clone();
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client.request(Method::GET, "/sse-strict").build();
    let stream_response = client.execute(request).await.unwrap();
    let mut chunks = stream_response.sse_chunks::<TestChunk>();

    let first = chunks.next().await.unwrap().unwrap();
    assert_eq!(first, SseChunk::Data(TestChunk { value: 1 }));

    let error = chunks.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseDecode);
    assert_eq!(error.log_redactor.policy(), &expected_policy);
}

#[tokio::test]
async fn test_execute_stream_decode_events_uses_client_default_sse_limits() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![(
            "Content-Type".to_string(),
            "text/event-stream".to_string(),
        )],
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
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client.request(Method::GET, "/sse-limits").build();
    let stream_response = client.execute(request).await.unwrap();
    let mut events = stream_response.sse_messages();

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert!(error.message.contains("max_frame_bytes"));
}

/// Verifies SSE decoding errors retain the client redactor snapshot instead of
/// rebuilding a default redactor after the response has been created.
#[tokio::test]
async fn test_sse_decode_error_preserves_client_redactor_policy() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![(
            "Content-Type".to_string(),
            "text/event-stream".to_string(),
        )],
        chunks: vec![ResponseChunk {
            delay: Duration::from_millis(0),
            bytes: vec![0xFF, b'\n'],
        }],
        finish: true,
    })
    .await;

    let builder = RedactionPolicy::default()
        .to_builder()
        .http(|http| {
            let _ = http
                .query()
                .raise("tenant_stream_secret", Sensitivity::Secret);
        })
        .expect("the test policy input should be valid");
    let expected_policy = builder
        .build()
        .expect("the custom HTTP policy should be valid");
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.log_redaction_policy = expected_policy.clone();
    let client = HttpClientFactory::new()
        .create(options)
        .expect("the client should be created");

    let request = client.request(Method::GET, "/sse-invalid-utf8").build();
    let response = client
        .execute(request)
        .await
        .expect("the response should open");
    let mut events = response.sse_messages();
    let error = events
        .next()
        .await
        .expect("the stream should yield one decode error")
        .expect_err("the invalid UTF-8 line should fail SSE decoding");

    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert_eq!(error.log_redactor.policy(), &expected_policy);
    server.finish().await;
}
