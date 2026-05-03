/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! # `SseReconnectRunner` integration tests
//!
//! Covers [`HttpClient::execute_sse_with_reconnect`](qubit_http::HttpClient::execute_sse_with_reconnect).

use std::io::Error as IoError;
use std::sync::{
    atomic::{
        AtomicUsize,
        Ordering,
    },
    Arc,
    Mutex,
};
use std::time::{
    Duration,
    Instant,
};

use futures_util::StreamExt;
use http::Method;
use qubit_http::{
    sse::SseReconnectOptions,
    CancellationToken,
    HttpClientFactory,
    HttpClientOptions,
    HttpError,
    HttpErrorKind,
    HttpRequestInterceptor,
    RetryDelay,
    RetryJitter,
    RetryOptions,
};
use tokio::time::timeout;

use crate::common::{
    spawn_multi_shot_server,
    spawn_one_shot_server,
    ResponseChunk,
    ResponsePlan,
};

/// Builds retry options for SSE reconnect tests.
///
/// # Parameters
/// - `max_reconnects`: Maximum reconnect attempts after the initial attempt.
/// - `delay`: Delay strategy for reconnect attempts.
/// - `jitter`: Jitter strategy for reconnect delays.
///
/// # Returns
/// Retry options with `max_attempts = max_reconnects + 1`.
fn build_retry_options(
    max_reconnects: u32,
    delay: RetryDelay,
    jitter: RetryJitter,
) -> RetryOptions {
    RetryOptions::new(max_reconnects + 1, None, None, delay, jitter)
        .expect("SSE reconnect test retry options should be valid")
}

/// Builds retry options for SSE reconnect tests with max elapsed-time limit.
///
/// # Parameters
/// - `max_reconnects`: Maximum reconnect attempts after the initial attempt.
/// - `max_elapsed`: Maximum elapsed reconnect duration.
/// - `delay`: Delay strategy for reconnect attempts.
/// - `jitter`: Jitter strategy for reconnect delays.
///
/// # Returns
/// Retry options with `max_attempts = max_reconnects + 1`.
fn build_retry_options_with_max_elapsed(
    max_reconnects: u32,
    max_elapsed: Duration,
    delay: RetryDelay,
    jitter: RetryJitter,
) -> RetryOptions {
    RetryOptions::new(max_reconnects + 1, None, Some(max_elapsed), delay, jitter)
        .expect("SSE reconnect test retry options should be valid")
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_propagates_last_event_id() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"id: evt-1\ndata: first\n\n".to_vec(),
            }],
            finish: false,
        },
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"data: second\n\n".to_vec(),
            }],
            finish: true,
        },
    ])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client.request(Method::GET, "/sse-reconnect").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                1,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );
    let first = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "first");
    assert_eq!(first.id.as_deref(), Some("evt-1"));
    let second = events.next().await.unwrap().unwrap();
    assert_eq!(second.data, "second");
    assert!(events.next().await.is_none());

    let requests = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].target, "/sse-reconnect");
    assert_eq!(requests[1].target, "/sse-reconnect");
    assert_eq!(
        requests[1].headers.get("last-event-id"),
        Some(&"evt-1".to_string())
    );
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_honors_server_retry_delay() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"id: evt-2\nretry: 120\ndata: first\n\n".to_vec(),
            }],
            finish: false,
        },
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"data: second\n\n".to_vec(),
            }],
            finish: true,
        },
    ])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new().create(options).unwrap();

    let start = Instant::now();
    let request = client.request(Method::GET, "/sse-retry-delay").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                1,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );
    assert_eq!(events.next().await.unwrap().unwrap().data, "first");
    assert_eq!(events.next().await.unwrap().unwrap().data, "second");
    assert!(events.next().await.is_none());
    let elapsed = start.elapsed();
    assert!(
        elapsed >= Duration::from_millis(100),
        "elapsed={elapsed:?} should honor SSE retry delay"
    );

    let requests = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(requests.len(), 2);
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_server_retry_overrides_once_and_preserves_backoff_progression(
) {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"retry: 120\ndata: first\n\n".to_vec(),
            }],
            finish: false,
        },
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"data: second\n\n".to_vec(),
            }],
            finish: false,
        },
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"data: done\n\n".to_vec(),
            }],
            finish: true,
        },
    ])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let mut client = HttpClientFactory::new().create(options).unwrap();

    let request_starts = Arc::new(Mutex::new(Vec::new()));
    let request_starts_for_interceptor = Arc::clone(&request_starts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request| {
        request_starts_for_interceptor
            .lock()
            .expect("request_starts mutex should not be poisoned")
            .push(Instant::now());
        Ok(())
    }));

    let request = client
        .request(Method::GET, "/sse-server-retry-once-then-backoff")
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                2,
                RetryDelay::exponential(Duration::from_millis(40), Duration::from_millis(200), 2.0),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: true,
            apply_jitter_to_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );
    assert_eq!(events.next().await.unwrap().unwrap().data, "first");
    assert_eq!(events.next().await.unwrap().unwrap().data, "second");
    assert_eq!(events.next().await.unwrap().unwrap().data, "done");
    assert!(events.next().await.is_none());

    let requests = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(requests.len(), 3);

    let starts = request_starts
        .lock()
        .expect("request_starts mutex should not be poisoned");
    assert_eq!(starts.len(), 3);
    let first_reconnect_delay = starts[1].saturating_duration_since(starts[0]);
    let second_reconnect_delay = starts[2].saturating_duration_since(starts[1]);
    assert!(
        first_reconnect_delay >= Duration::from_millis(95),
        "first reconnect should honor server retry: {first_reconnect_delay:?}"
    );
    assert!(
        first_reconnect_delay <= Duration::from_millis(220),
        "first reconnect delay should stay near 120ms: {first_reconnect_delay:?}"
    );
    assert!(
        second_reconnect_delay >= Duration::from_millis(55),
        "second reconnect should follow local backoff progression: {second_reconnect_delay:?}"
    );
    assert!(
        second_reconnect_delay <= Duration::from_millis(150),
        "second reconnect delay should stay near 80ms: {second_reconnect_delay:?}"
    );
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_caps_server_retry_delay() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"retry: 800\ndata: first\n\n".to_vec(),
            }],
            finish: false,
        },
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"data: second\n\n".to_vec(),
            }],
            finish: true,
        },
    ])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let mut client = HttpClientFactory::new().create(options).unwrap();

    let request_starts = Arc::new(Mutex::new(Vec::new()));
    let request_starts_for_interceptor = Arc::clone(&request_starts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request| {
        request_starts_for_interceptor
            .lock()
            .expect("request_starts mutex should not be poisoned")
            .push(Instant::now());
        Ok(())
    }));

    let request = client.request(Method::GET, "/sse-server-retry-cap").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                1,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: true,
            server_retry_max_delay: Some(Duration::from_millis(80)),
            apply_jitter_to_server_retry: false,
        },
    );

    assert_eq!(events.next().await.unwrap().unwrap().data, "first");
    assert_eq!(events.next().await.unwrap().unwrap().data, "second");
    assert!(events.next().await.is_none());

    let requests = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(requests.len(), 2);

    let starts = request_starts
        .lock()
        .expect("request_starts mutex should not be poisoned");
    assert_eq!(starts.len(), 2);
    let reconnect_delay = starts[1].saturating_duration_since(starts[0]);
    assert!(
        reconnect_delay >= Duration::from_millis(65),
        "reconnect delay should honor server retry cap lower bound: {reconnect_delay:?}"
    );
    assert!(
        reconnect_delay < Duration::from_millis(220),
        "reconnect delay should be capped instead of waiting near 800ms: {reconnect_delay:?}"
    );
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_derives_server_retry_cap_from_delay_strategy() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"retry: 800\ndata: first\n\n".to_vec(),
            }],
            finish: true,
        },
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"data: second\n\n".to_vec(),
            }],
            finish: true,
        },
    ])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let mut client = HttpClientFactory::new().create(options).unwrap();

    let request_starts = Arc::new(Mutex::new(Vec::new()));
    let request_starts_for_interceptor = Arc::clone(&request_starts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request| {
        request_starts_for_interceptor
            .lock()
            .expect("request_starts mutex should not be poisoned")
            .push(Instant::now());
        Ok(())
    }));

    let request = client
        .request(Method::GET, "/sse-server-retry-derived-cap")
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                1,
                RetryDelay::Random {
                    min: Duration::from_millis(20),
                    max: Duration::from_millis(80),
                },
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: true,
            apply_jitter_to_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );

    assert_eq!(events.next().await.unwrap().unwrap().data, "first");
    assert_eq!(events.next().await.unwrap().unwrap().data, "second");
    let _ = events.next().await;

    let requests = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(requests.len(), 2);

    let starts = request_starts
        .lock()
        .expect("request_starts mutex should not be poisoned");
    assert_eq!(starts.len(), 2);
    let reconnect_delay = starts[1].saturating_duration_since(starts[0]);
    assert!(
        reconnect_delay >= Duration::from_millis(65),
        "server retry delay should be capped by random max: {reconnect_delay:?}"
    );
    assert!(
        reconnect_delay < Duration::from_millis(220),
        "derived cap should avoid waiting near 800ms: {reconnect_delay:?}"
    );
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_can_disable_server_retry_jitter() {
    let reconnect_count: usize = 8;
    let mut plans = Vec::with_capacity(reconnect_count + 1);
    for index in 0..reconnect_count {
        plans.push(ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: format!("retry: 120\ndata: tick-{index}\n\n").into_bytes(),
            }],
            finish: false,
        });
    }
    plans.push(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![ResponseChunk {
            delay: Duration::from_millis(0),
            bytes: b"data: done\n\n".to_vec(),
        }],
        finish: true,
    });
    let server = spawn_multi_shot_server(plans).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let mut client = HttpClientFactory::new().create(options).unwrap();

    let request_starts = Arc::new(Mutex::new(Vec::new()));
    let request_starts_for_interceptor = Arc::clone(&request_starts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request| {
        request_starts_for_interceptor
            .lock()
            .expect("request_starts mutex should not be poisoned")
            .push(Instant::now());
        Ok(())
    }));

    let request = client
        .request(Method::GET, "/sse-disable-server-retry-jitter")
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                reconnect_count as u32,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::factor(1.0),
            ),
            reconnect_on_eof: true,
            honor_server_retry: true,
            server_retry_max_delay: Some(Duration::from_millis(120)),
            apply_jitter_to_server_retry: false,
        },
    );

    for index in 0..reconnect_count {
        let event = events
            .next()
            .await
            .expect("tick event should be present")
            .expect("tick event should decode");
        assert_eq!(event.data, format!("tick-{index}"));
    }
    let final_event = events
        .next()
        .await
        .expect("final done event should be present")
        .expect("final done event should decode");
    assert_eq!(final_event.data, "done");
    assert!(events.next().await.is_none());

    let requests = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(requests.len(), reconnect_count + 1);

    let starts = request_starts
        .lock()
        .expect("request_starts mutex should not be poisoned");
    assert_eq!(starts.len(), reconnect_count + 1);
    for (index, pair) in starts.windows(2).enumerate() {
        let reconnect_delay = pair[1].saturating_duration_since(pair[0]);
        assert!(
            reconnect_delay >= Duration::from_millis(95),
            "reconnect #{index} should not be shortened by jitter: {reconnect_delay:?}"
        );
        assert!(
            reconnect_delay <= Duration::from_millis(230),
            "reconnect #{index} should stay near configured server retry delay: {reconnect_delay:?}"
        );
    }
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_respects_retry_max_elapsed() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 500,
            headers: vec![],
            body: b"server-error-1".to_vec(),
        },
        ResponsePlan::Immediate {
            status: 500,
            headers: vec![],
            body: b"server-error-2".to_vec(),
        },
    ])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client.request(Method::GET, "/sse-max-elapsed").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options_with_max_elapsed(
                5,
                Duration::from_millis(80),
                RetryDelay::fixed(Duration::from_millis(60)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );

    let error = events
        .next()
        .await
        .expect("max_elapsed exhaustion should surface an error item")
        .expect_err("stream should fail when max_elapsed is exhausted");
    assert_eq!(error.kind, HttpErrorKind::RetryMaxElapsedExceeded);
    assert_eq!(error.status, Some(http::StatusCode::INTERNAL_SERVER_ERROR));
    assert!(
        error.source.is_some(),
        "last retryable error should be chained"
    );
    assert!(
        error
            .message
            .contains("SSE reconnect max duration exceeded"),
        "error message should mention max elapsed budget: {}",
        error.message
    );
    assert!(
        error.message.contains("last retryable error"),
        "error message should preserve last error context: {}",
        error.message
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_checks_max_elapsed_before_eof_reconnect_sleep() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: Vec::new(),
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client
        .request(Method::GET, "/sse-max-elapsed-before-eof-reconnect")
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options_with_max_elapsed(
                5,
                Duration::from_millis(100),
                RetryDelay::fixed(Duration::from_millis(150)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );

    let error = events
        .next()
        .await
        .expect("reconnect should stop with max_elapsed error")
        .expect_err("max_elapsed should block reconnect sleep before second request");
    assert_eq!(error.kind, HttpErrorKind::RetryMaxElapsedExceeded);
    assert!(
        error
            .message
            .contains("SSE reconnect max duration exceeded"),
        "error message should mention max elapsed budget: {}",
        error.message
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse-max-elapsed-before-eof-reconnect");
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_sleep_can_be_cancelled() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: Vec::new(),
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new().create(options).unwrap();

    let token = CancellationToken::new();
    let token_for_task = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        token_for_task.cancel();
    });

    let request = client
        .request(Method::GET, "/sse-cancel-reconnect-sleep")
        .cancellation_token(token)
        .build();

    let start = Instant::now();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                1,
                RetryDelay::fixed(Duration::from_secs(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );
    let error = events
        .next()
        .await
        .expect("cancelled reconnect should emit one error item")
        .expect_err("cancelled reconnect sleep should fail");
    let elapsed = start.elapsed();
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(
        elapsed < Duration::from_millis(500),
        "elapsed={elapsed:?} should fail fast on cancellation"
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse-cancel-reconnect-sleep");
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_disables_inner_http_retry() {
    let mut options = HttpClientOptions::default();
    options
        .set_base_url("http://127.0.0.1:18080")
        .expect("base URL should parse");
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.delay_strategy = RetryDelay::None;
    let mut client = HttpClientFactory::new().create(options).unwrap();

    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request| {
        attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Err(HttpError::transport("injector transient transport failure"))
    }));

    let request = client
        .request(Method::GET, "/sse-disable-inner-retry")
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                1,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );

    let error = events
        .next()
        .await
        .expect("stream should yield one failure item")
        .expect_err("transport failure should stop after reconnect budget is exhausted");
    assert_eq!(error.kind, HttpErrorKind::Transport);
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        2,
        "inner retry must be disabled; only outer reconnect attempts should run"
    );
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_fails_fast_on_non_sse_content_type() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        body: b"plain-text".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client
        .request(Method::GET, "/sse-content-type-check")
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                3,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );

    let error = events
        .next()
        .await
        .expect("non-SSE response should emit an error item")
        .expect_err("non-SSE content type should fail fast");
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert!(error.message.contains("text/event-stream"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse-content-type-check");
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_fails_fast_on_missing_content_type() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"data: ignored\n\n".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client
        .request(Method::GET, "/sse-missing-content-type")
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                3,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );

    let error = events
        .next()
        .await
        .expect("missing content type should emit an error item")
        .expect_err("missing content type should fail fast");
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert!(error.message.contains("Missing Content-Type"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse-missing-content-type");
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_fails_fast_on_non_utf8_content_type() {
    let server = spawn_one_shot_server(ResponsePlan::ImmediateRawHeaders {
        status: 200,
        headers: vec![("Content-Type".to_string(), vec![0xFF, 0xFE])],
        body: b"data: ignored\n\n".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client
        .request(Method::GET, "/sse-non-utf8-content-type")
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                3,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );

    let error = events
        .next()
        .await
        .expect("non-UTF8 content type should emit an error item")
        .expect_err("non-UTF8 content type should fail fast");
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert!(error.message.contains("non-UTF8 Content-Type"));
    assert_eq!(error.method, Some(Method::GET));
    assert!(error
        .url
        .as_ref()
        .is_some_and(|url| url.path() == "/sse-non-utf8-content-type"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse-non-utf8-content-type");
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_rejects_content_type_prefix_collision() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![(
            "Content-Type".to_string(),
            "text/event-streaming; charset=utf-8".to_string(),
        )],
        body: b"plain-text".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client
        .request(Method::GET, "/sse-content-type-prefix-collision")
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                3,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );

    let error = events
        .next()
        .await
        .expect("invalid SSE media type should emit an error item")
        .expect_err("content-type prefix collision should fail fast");
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert!(error.message.contains("text/event-stream"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse-content-type-prefix-collision");
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_uses_custom_backoff_parameters() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: Vec::new(),
            finish: true,
        },
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: Vec::new(),
            finish: true,
        },
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"data: done\n\n".to_vec(),
            }],
            finish: true,
        },
    ])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new().create(options).unwrap();

    let start = Instant::now();
    let request = client.request(Method::GET, "/sse-custom-backoff").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                2,
                RetryDelay::exponential(Duration::from_millis(80), Duration::from_millis(200), 3.0),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );
    assert_eq!(events.next().await.unwrap().unwrap().data, "done");
    assert!(events.next().await.is_none());
    let elapsed = start.elapsed();
    assert!(
        elapsed >= Duration::from_millis(220),
        "elapsed={elapsed:?} should reflect custom reconnect backoff settings"
    );

    let requests = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(requests.len(), 3);
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_does_not_retry_non_retryable_protocol_error() {
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

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let mut client = HttpClientFactory::new().create(options).unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request| {
        attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }));

    let request = client.request(Method::GET, "/sse-protocol-error").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                3,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert_eq!(attempts.load(Ordering::Relaxed), 1);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse-protocol-error");
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_reports_invalid_last_event_id_header_value() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![ResponseChunk {
            delay: Duration::from_millis(0),
            bytes: b"id: bad\x7fvalue\ndata: first\n\n".to_vec(),
        }],
        finish: false,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client
        .request(Method::GET, "/sse-invalid-last-event-id")
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                1,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );

    let first = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "first");

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("Invalid Last-Event-ID"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse-invalid-last-event-id");
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_retries_on_unexpected_eof_message() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![ResponseChunk {
            delay: Duration::from_millis(0),
            bytes: b"data: recovered\n\n".to_vec(),
        }],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let mut client = HttpClientFactory::new().create(options).unwrap();

    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request| {
        let current = attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        if current == 0 {
            Err(HttpError::other(
                "unexpected eof while preparing local SSE pipeline",
            ))
        } else {
            Ok(())
        }
    }));

    let request = client.request(Method::GET, "/sse-unexpected-eof").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                1,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: false,
            honor_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );

    let first = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "recovered");
    assert!(events.next().await.is_none());
    assert_eq!(attempts.load(Ordering::Relaxed), 2);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse-unexpected-eof");
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_retries_on_unexpected_eof_source_message() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![ResponseChunk {
            delay: Duration::from_millis(0),
            bytes: b"data: recovered-from-source\n\n".to_vec(),
        }],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.write_timeout = Duration::from_secs(2);
    let mut client = HttpClientFactory::new().create(options).unwrap();

    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request| {
        let current = attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        if current == 0 {
            Err(HttpError::other("local SSE source failure")
                .with_source(IoError::other("unexpected eof from wrapped source")))
        } else {
            Ok(())
        }
    }));

    let request = client
        .request(Method::GET, "/sse-unexpected-eof-source")
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                1,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: false,
            honor_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );

    let first = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "recovered-from-source");
    assert!(events.next().await.is_none());
    assert_eq!(attempts.load(Ordering::Relaxed), 2);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse-unexpected-eof-source");
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_does_not_retry_non_eof_source_error() {
    let mut options = HttpClientOptions::default();
    options
        .set_base_url("http://127.0.0.1:18080")
        .expect("base URL should parse");
    options.timeouts.read_timeout = Duration::from_secs(1);
    options.timeouts.write_timeout = Duration::from_secs(1);
    let mut client = HttpClientFactory::new().create(options).unwrap();

    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request| {
        attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Err(HttpError::other("local SSE non-EOF failure")
            .with_source(IoError::other("ordinary source error")))
    }));

    let request = client.request(Method::GET, "/sse-non-eof-source").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                3,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn test_execute_sse_with_reconnect_does_not_retry_cancelled_error() {
    let mut options = HttpClientOptions::default();
    options
        .set_base_url("http://127.0.0.1:18080")
        .expect("base URL should parse");
    options.timeouts.read_timeout = Duration::from_secs(1);
    options.timeouts.write_timeout = Duration::from_secs(1);
    let mut client = HttpClientFactory::new().create(options).unwrap();

    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request| {
        attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Err(HttpError::cancelled("cancelled before SSE send"))
    }));

    let request = client.request(Method::GET, "/sse-cancelled").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                3,
                RetryDelay::fixed(Duration::from_millis(1)),
                RetryJitter::None,
            ),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
}
