/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # `SseReconnectRunner` integration tests
//!
//! Covers [`HttpClient::execute_sse_with_reconnect`](qubit_http::HttpClient::execute_sse_with_reconnect).

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use http::Method;
use qubit_http::{
    sse::SseReconnectOptions, CancellationToken, HttpClientFactory, HttpClientOptions, HttpError,
    HttpErrorKind, HttpRequestInterceptor, RetryDelay, RetryJitter, RetryOptions,
};
use tokio::time::timeout;

use crate::common::{spawn_multi_shot_server, spawn_one_shot_server, ResponseChunk, ResponsePlan};

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
    RetryOptions::new(max_reconnects + 1, None, delay, jitter)
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
    RetryOptions::new(max_reconnects + 1, Some(max_elapsed), delay, jitter)
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
        ResponsePlan::Immediate {
            status: 500,
            headers: vec![],
            body: b"server-error-3".to_vec(),
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
        },
    );

    let error = events
        .next()
        .await
        .expect("max_elapsed exhaustion should surface an error item")
        .expect_err("stream should fail when max_elapsed is exhausted");
    assert_eq!(error.kind, HttpErrorKind::Status);
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
    assert_eq!(captured.len(), 3);
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

    let request = client.request(Method::GET, "/sse-disable-inner-retry").build();
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
async fn test_execute_sse_with_reconnect_falls_back_when_jitter_invalid() {
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
            chunks: vec![ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"data: recovered\n\n".to_vec(),
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
    let mut invalid_retry = build_retry_options(
        1,
        RetryDelay::fixed(Duration::from_millis(5)),
        RetryJitter::None,
    );
    invalid_retry.jitter = RetryJitter::factor(f64::NAN);

    let request = client.request(Method::GET, "/sse-invalid-jitter").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: invalid_retry,
            reconnect_on_eof: true,
            honor_server_retry: false,
        },
    );

    let first = events.next().await.unwrap().unwrap();
    assert_eq!(first.data, "recovered");
    assert!(events.next().await.is_none());

    let requests = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(requests.len(), 2);
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
        },
    );

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
}
