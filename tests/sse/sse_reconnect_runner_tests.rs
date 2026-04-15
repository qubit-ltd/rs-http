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
    sse::SseReconnectOptions, HttpClientFactory, HttpClientOptions, HttpError, HttpErrorKind,
    HttpRequestInterceptor,
};
use tokio::time::timeout;

use crate::common::{spawn_multi_shot_server, spawn_one_shot_server, ResponseChunk, ResponsePlan};

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
            max_reconnects: 1,
            reconnect_delay: Duration::from_millis(1),
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
            max_reconnects: 1,
            reconnect_delay: Duration::from_millis(1),
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
            max_reconnects: 2,
            reconnect_delay: Duration::from_millis(80),
            max_reconnect_delay: Duration::from_millis(200),
            reconnect_backoff_multiplier: 3.0,
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
            max_reconnects: 3,
            reconnect_delay: Duration::from_millis(1),
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
            max_reconnects: 1,
            reconnect_delay: Duration::from_millis(1),
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
            max_reconnects: 1,
            reconnect_delay: Duration::from_millis(1),
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
            max_reconnects: 3,
            reconnect_delay: Duration::from_millis(1),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
}
