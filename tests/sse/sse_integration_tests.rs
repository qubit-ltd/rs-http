// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error as _;
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use http::HeaderMap;
use http::Method;
use http::StatusCode;
use qubit_budget::json::JsonValueLimits;
use qubit_http::sse::DoneMarkerPolicy;
use qubit_http::sse::SseChunk;
use qubit_http::sse::SseCompletionPolicy;
use qubit_http::sse::SseJsonMode;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use qubit_http::HttpResponse;
use qubit_redact::RedactionPolicy;
use qubit_redact::Sensitivity;
use tokio::test as tokio_test;
use tokio::time::timeout;

use crate::common::spawn_one_shot_server;
use crate::common::ResponseChunk;
use crate::common::ResponsePlan;

#[derive(Debug, serde::Deserialize, PartialEq, Eq)]
struct TestChunk {
    value: i32,
}

fn buffered_response_from_parts(chunks: Vec<Vec<u8>>) -> HttpResponse {
    let body = chunks.into_iter().flatten().collect::<Vec<u8>>();
    HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from(body),
        url::Url::parse("https://example.com/stream?access_token=SSE_PRIVATE").unwrap(),
        Method::GET,
    )
}

#[tokio_test]
async fn test_decode_events_reports_sse_protocol_error_on_non_utf8_line() {
    let response = buffered_response_from_parts(vec![vec![0xFF, b'\n']]);
    let mut events = response.sse_messages();
    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert_eq!(error.method(), Some(&Method::GET));
    assert_eq!(error.status(), Some(StatusCode::OK));
    assert_eq!(
        error.url().unwrap().query(),
        Some("access_token=SSE_PRIVATE")
    );
    assert!(!error.to_string().contains("SSE_PRIVATE"));
    assert!(!format!("{error:?}").contains("SSE_PRIVATE"));
}

#[tokio_test]
async fn test_decode_events_decodes_buffered_body_with_complete_frames() {
    let response = buffered_response_from_parts(vec![
        b"data: {\"val".to_vec(),
        b"ue\":1}\n".to_vec(),
        b"\n".to_vec(),
        b"data: {\"value\":2}\n\n".to_vec(),
    ]);
    let mut events = response.sse_messages();

    let first = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "{\"value\":1}");

    let second = events.next().await.unwrap().unwrap();
    assert_eq!(second.data, "{\"value\":2}");
}

#[tokio_test]
async fn test_decode_events_reports_frame_limit_error() {
    let response = buffered_response_from_parts(vec![b"data: one\ndata: two\n\n".to_vec()]);
    let mut events = response
        .sse_max_line_bytes(1024)
        .sse_max_frame_bytes(8)
        .sse_messages();
    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert_eq!(error.method(), Some(&Method::GET));
    assert_eq!(error.status(), Some(StatusCode::OK));
    assert_eq!(
        error.url().unwrap().query(),
        Some("access_token=SSE_PRIVATE")
    );
    assert!(!format!("{error:?}").contains("SSE_PRIVATE"));
}

#[tokio_test]
async fn test_decode_events_reports_line_limit_with_http_context() {
    let response = buffered_response_from_parts(vec![b"data: too-long\n\n".to_vec()]);
    let mut events = response.sse_max_line_bytes(4).sse_messages();
    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert_eq!(error.method(), Some(&Method::GET));
    assert_eq!(error.status(), Some(StatusCode::OK));
    assert_eq!(
        error.url().unwrap().query(),
        Some("access_token=SSE_PRIVATE")
    );
    assert!(!format!("{error:?}").contains("SSE_PRIVATE"));
}

#[tokio_test]
async fn test_sse_missing_done_marker_error_has_http_context() {
    let response = buffered_response_from_parts(vec![b"data: {\"value\":1}\n\n".to_vec()]);
    let mut chunks = response
        .sse_done_marker_policy(DoneMarkerPolicy::Custom("[DONE]".to_string()))
        .sse_completion_policy(SseCompletionPolicy::RequireDoneMarker)
        .sse_chunks::<TestChunk>();
    assert_eq!(
        chunks.next().await.unwrap().unwrap(),
        SseChunk::Data(TestChunk { value: 1 })
    );
    let error = chunks.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert_eq!(error.method(), Some(&Method::GET));
    assert_eq!(error.status(), Some(StatusCode::OK));
    assert_eq!(
        error.url().unwrap().query(),
        Some("access_token=SSE_PRIVATE")
    );
    assert!(!format!("{error:?}").contains("SSE_PRIVATE"));
}

/// Regression: `sse_max_line_bytes` → `sse_max_frame_bytes` → `sse_messages()`
/// must compile and apply limits from the same chain (see user guide “Configure
/// `sse_messages` options”).
#[tokio_test]
async fn test_regression_sse_messages_chain_setters_before_decode() {
    let response = buffered_response_from_parts(vec![b"data: ok\n\n".to_vec()]);
    let mut events = response
        .sse_max_line_bytes(64 * 1024)
        .sse_max_frame_bytes(1024 * 1024)
        .sse_messages();
    let ev = events.next().await.unwrap().unwrap();
    assert_eq!(ev.data, "ok");
    assert!(
        events.next().await.is_none(),
        "the SSE stream should be exhausted after the complete frame"
    );
}

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client.request(Method::GET, "/sse").build();
    let stream_response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let mut events = stream_response.sse_messages();

    let first = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "{\"value\":1}");
    let second = events.next().await.unwrap().unwrap();
    assert_eq!(second.data, "{\"value\":2}");
    assert!(
        events.next().await.is_none(),
        "the SSE stream should be exhausted after both events"
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse");
}

#[tokio_test]
async fn test_loopback_many_heartbeat_frames_reaches_final_event() {
    let mut body = String::new();
    for _ in 0..4_096 {
        body.push_str(": heartbeat\n\n");
    }
    body.push_str("data: ok\n\n");
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        body: body.into_bytes(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client.request(Method::GET, "/sse-heartbeat-audit").build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("loopback execute timed out")
        .unwrap();
    let mut events = response.sse_messages();

    let event = timeout(Duration::from_secs(3), events.next())
        .await
        .expect("loopback SSE decode timed out")
        .unwrap()
        .unwrap();
    assert_eq!(event.data, "ok");
    assert!(events.next().await.is_none());
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("loopback server finish timed out");
    assert_eq!(captured.target, "/sse-heartbeat-audit");
}

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(1);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client.request(Method::GET, "/sse-timeout").build();
    let stream_response = client.execute(request).await.unwrap();
    let mut events = stream_response.sse_messages();

    let first = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "{\"value\":1}");

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::ReadTimeout);
    assert_eq!(error.method, Some(Method::GET));
    assert_eq!(error.url.as_ref().map(url::Url::path), Some("/sse-timeout"));
    assert_eq!(error.status, Some(StatusCode::OK));
    assert!(
        error.source().is_none(),
        "timeout wrapper has no backend source"
    );
}

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client.request(Method::GET, "/sse-strict").build();
    let stream_response = client.execute(request).await.unwrap();
    let mut chunks = stream_response.sse_chunks::<TestChunk>();

    let first = chunks.next().await.unwrap().unwrap();
    assert_eq!(first, SseChunk::Data(TestChunk { value: 1 }));

    let error = chunks.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseDecode);
    assert_eq!(error.log_redactor.policy(), &expected_policy);
    assert_eq!(error.method(), Some(&Method::GET));
    assert_eq!(error.status(), Some(StatusCode::OK));
    assert_eq!(
        error.url(),
        Some(&server.base_url().join("/sse-strict").unwrap())
    );
    assert!(error.source.as_deref().is_some_and(|source| {
        source
            .downcast_ref::<qubit_json::decode::JsonDecodeError>()
            .is_some()
    }));
}

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client.request(Method::GET, "/sse-limits").build();
    let stream_response = client.execute(request).await.unwrap();
    let mut events = stream_response.sse_messages();

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert!(
        error.message.contains("max_frame_bytes"),
        "the protocol error should identify the configured frame limit"
    );
}

#[tokio_test]
async fn test_sse_chunks_apply_client_json_value_limits() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![ResponseChunk {
            delay: Duration::ZERO,
            bytes: b"data: {\"value\":1}\n\n".to_vec(),
        }],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.sse_json_mode = SseJsonMode::Strict;
    options.json_value_limits = JsonValueLimits::builder().max_nodes(1).build();
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");
    let request = client.request(Method::GET, "/sse-json-value-limit").build();
    let response = client.execute(request).await.expect("request succeeds");
    let mut chunks = response.sse_chunks::<TestChunk>();

    let error = chunks
        .next()
        .await
        .expect("stream should produce a result")
        .expect_err("configured JSON node limit must reject the SSE payload");

    assert_eq!(error.kind, HttpErrorKind::SseDecode);
    let captured = server.finish().await;
    assert_eq!(captured.target, "/sse-json-value-limit");
}

/// Verifies SSE decoding errors retain the client redactor snapshot instead of
/// rebuilding a default redactor after the response has been created.
#[tokio_test]
async fn test_sse_decode_error_preserves_client_redactor_policy() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
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
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
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
