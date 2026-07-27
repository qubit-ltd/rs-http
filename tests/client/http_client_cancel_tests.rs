// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use http::{Method, StatusCode};
use qubit_http::{
    AsyncHttpHeaderInjector, CancellationToken, HttpClientFactory, HttpClientOptions, HttpError,
    HttpErrorKind, RetryHint,
};
use qubit_retry::RetryDelay;
use tokio::time::timeout;

use crate::common::{spawn_multi_shot_server, spawn_one_shot_server, ResponseChunk, ResponsePlan};

#[test]
fn test_cancelled_error_semantics() {
    let error = HttpError::cancelled("request cancelled by caller");
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(error.retry_hint(), RetryHint::NonRetryable);
    assert!(error.message.contains("cancelled"));
}

#[tokio::test]
async fn test_execute_request_with_pre_cancelled_token_returns_cancelled_error() {
    let server = spawn_multi_shot_server(vec![]).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = CancellationToken::new();
    token.cancel();
    let request = client
        .request(Method::GET, "/pre-cancelled")
        .cancellation_token(token)
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("request should be cancelled");
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(error.message.contains("cancelled"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert!(captured.is_empty());
}

#[tokio::test]
async fn test_execute_request_with_pre_cancelled_token_skips_request_interceptors() {
    let server = spawn_multi_shot_server(vec![]).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let interceptor_calls = Arc::new(AtomicUsize::new(0));
    let interceptor_calls_clone = Arc::clone(&interceptor_calls);
    client.add_request_interceptor(qubit_http::HttpRequestInterceptor::new(
        move |_request: &mut qubit_http::HttpRequest| {
            interceptor_calls_clone.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
    ));

    let token = CancellationToken::new();
    token.cancel();
    let request = client
        .request(Method::GET, "/pre-cancelled-interceptor")
        .query_param("trace", "yes")
        .cancellation_token(token)
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("request should be cancelled");

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(interceptor_calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        error
            .url
            .as_ref()
            .expect("cancelled error should include request URL")
            .query(),
        Some("trace=yes")
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert!(captured.is_empty());
}

#[tokio::test]
async fn test_execute_request_cancelled_by_interceptor_stops_before_send() {
    let server = spawn_multi_shot_server(vec![]).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = CancellationToken::new();
    let interceptor_token = token.clone();
    client.add_request_interceptor(qubit_http::HttpRequestInterceptor::new(
        move |_request: &mut qubit_http::HttpRequest| {
            interceptor_token.cancel();
            Ok(())
        },
    ));

    let request = client
        .request(Method::GET, "/cancelled-by-interceptor")
        .cancellation_token(token)
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("interceptor cancellation should stop before send");

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(error.message.contains("before sending"));
    assert_eq!(
        error
            .url
            .as_ref()
            .expect("cancelled error should include request URL")
            .path(),
        "/cancelled-by-interceptor"
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert!(captured.is_empty());
}

#[tokio::test]
async fn test_execute_request_can_be_cancelled_while_preparing_async_headers() {
    let server = spawn_multi_shot_server(vec![]).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_secs(5);
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    client.add_async_header_injector(AsyncHttpHeaderInjector::new(
        |_headers: &mut http::HeaderMap| {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok(())
            })
        },
    ));

    let token = CancellationToken::new();
    let token_for_task = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        token_for_task.cancel();
    });

    let request = client
        .request(Method::GET, "/cancel-preparing-headers")
        .cancellation_token(token)
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("request should be cancelled while preparing headers");

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(error.message.contains("preparing request"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert!(captured.is_empty());
}

#[tokio::test]
async fn test_execute_request_can_be_cancelled_while_reading_response_body() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 200,
        headers: vec![],
        total_length: 16,
        prefix: b"abc".to_vec(),
        delay: Duration::from_secs(2),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(5);
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = CancellationToken::new();
    let token_for_task = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        token_for_task.cancel();
    });

    let request = client
        .request(Method::GET, "/cancel-reading")
        .cancellation_token(token)
        .build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should start");
    let error = response
        .bytes()
        .await
        .expect_err("request should be cancelled while reading body");
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(error.message.contains("cancelled"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/cancel-reading");
}

#[tokio::test]
async fn test_execute_request_can_be_cancelled_while_reading_status_error_preview() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 503,
        headers: vec![],
        chunks: vec![
            ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"partial".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_secs(1),
                bytes: b"later".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(5);
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = CancellationToken::new();
    let token_for_task = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        token_for_task.cancel();
    });

    let request = client
        .request(Method::GET, "/cancel-status-preview")
        .query_param("phase", "preview")
        .cancellation_token(token)
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("status error preview should be cancelled");

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(error.status, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert!(error.message.contains("status error response body preview"));
    assert_eq!(
        error
            .url
            .as_ref()
            .expect("cancelled error should include request URL")
            .query(),
        Some("phase=preview")
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/cancel-status-preview?phase=preview");
}

#[tokio::test]
async fn test_execute_retry_sleep_can_be_cancelled() {
    let server = spawn_multi_shot_server(vec![ResponsePlan::Immediate {
        status: 503,
        headers: vec![],
        body: b"retry later".to_vec(),
    }])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.delay_strategy = RetryDelay::Fixed(Duration::from_secs(5));
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = CancellationToken::new();
    let token_for_task = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        token_for_task.cancel();
    });

    let request = client
        .request(Method::GET, "/cancel-retry-sleep")
        .cancellation_token(token)
        .build();
    let started_at = Instant::now();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("request should be cancelled during retry sleep");

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(error.message.contains("waiting before next attempt"));
    assert!(
        started_at.elapsed() < Duration::from_secs(1),
        "retry cancellation should not wait for the whole backoff"
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].target, "/cancel-retry-sleep");
}

#[tokio::test]
async fn test_execute_request_can_be_cancelled_while_sending() {
    let server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_secs(2),
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_secs(5);
    options.timeouts.read_timeout = Duration::from_secs(5);
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = CancellationToken::new();
    let token_for_task = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        token_for_task.cancel();
    });

    let request = client
        .request(Method::GET, "/cancel-sending")
        .cancellation_token(token)
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("request should be cancelled while sending");
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(error.message.contains("sending"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/cancel-sending");
}

#[tokio::test]
async fn test_execute_stream_body_can_be_cancelled_after_first_chunk() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        chunks: vec![
            ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"first".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_secs(2),
                bytes: b"second".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(5);
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = CancellationToken::new();
    let request = client
        .request(Method::GET, "/cancel-stream")
        .cancellation_token(token.clone())
        .build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should start");

    let mut stream = response.stream().expect("stream body should be available");
    let first = stream
        .next()
        .await
        .expect("first stream item should exist")
        .expect("first stream item should be ok");
    assert_eq!(first, b"first".as_slice());

    token.cancel();

    let cancelled = stream
        .next()
        .await
        .expect("second stream item should exist")
        .expect_err("second stream item should be cancelled");
    assert_eq!(cancelled.kind, HttpErrorKind::Cancelled);
    assert!(cancelled.message.contains("cancelled"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/cancel-stream");
}

#[tokio::test]
async fn test_sse_messages_reports_pre_cancelled_stream_before_reading_body() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![],
        finish: false,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(5);
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = CancellationToken::new();
    let request = client
        .request(Method::GET, "/cancel-sse-events-before-read")
        .cancellation_token(token.clone())
        .build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should start");
    token.cancel();

    let mut events = response.sse_messages();
    let error = events
        .next()
        .await
        .expect("pre-cancelled SSE message stream should yield one error")
        .expect_err("SSE message stream should fail before reading body");

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(error.message.contains("before reading response body"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/cancel-sse-events-before-read");
}

#[tokio::test]
async fn test_sse_chunks_reports_pre_cancelled_stream_before_reading_body() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![],
        finish: false,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(5);
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = CancellationToken::new();
    let request = client
        .request(Method::GET, "/cancel-sse-chunks-before-read")
        .cancellation_token(token.clone())
        .build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should start");
    token.cancel();

    let mut chunks = response.sse_chunks::<serde_json::Value>();
    let error = chunks
        .next()
        .await
        .expect("pre-cancelled SSE chunk stream should yield one error")
        .expect_err("SSE chunk stream should fail before reading body");

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(error.message.contains("before reading response body"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/cancel-sse-chunks-before-read");
}
