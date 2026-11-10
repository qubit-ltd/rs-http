// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # `SseReconnectRunner` integration tests
//!
//! Covers [`HttpClient::execute_sse_with_reconnect`](qubit_http::HttpClient::execute_sse_with_reconnect).

use std::future::Future;
use std::io::Error as IoError;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use futures_util::StreamExt;
use http::HeaderName;
use http::Method;
use http::StatusCode;
use qubit_http::AsyncHttpHeaderInjector;
use qubit_http::HttpCancellationToken;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpError;
use qubit_http::HttpErrorKind;
use qubit_http::HttpHeaderInjector;
use qubit_http::HttpRequest;
use qubit_http::HttpRequestInterceptor;
use qubit_http::HttpRequestRetryOverride;
use qubit_http::HttpResponseInterceptor;
use qubit_http::HttpResponseInterceptorContext;
use qubit_http::sse::SseReconnectOptions;
use qubit_retry::BackoffPolicy;
use qubit_retry::RetryPolicy;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::pin;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::test as tokio_test;
use tokio::time::timeout;
use url::Url;

use crate::common::CapturedRequest;
use crate::common::ResponseChunk;
use crate::common::ResponsePlan;
use crate::common::spawn_multi_shot_server;
use crate::common::spawn_one_shot_server;

#[tokio_test]
async fn test_sse_reconnect_replays_post_after_eof_with_http_retry_disabled() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 200,
            headers: vec![("Content-Type".to_owned(), "text/event-stream".to_owned())],
            body: b"data: first\n\n".to_vec(),
        },
        ResponsePlan::Immediate {
            status: 204,
            headers: vec![],
            body: Vec::new(),
        },
    ])
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = false;
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");
    let request = client
        .request(Method::POST, "/events")
        .bytes_body(b"subscription".as_slice())
        .disable_retry()
        .build();
    let reconnect = SseReconnectOptions {
        reconnect_on_eof: true,
        retry: RetryPolicy::builder()
            .max_attempts(2)
            .backoff(BackoffPolicy::immediate())
            .build()
            .expect("retry policy should be valid"),
        ..SseReconnectOptions::default()
    };
    let mut events = client.execute_sse_with_reconnect(request, reconnect);
    let first = timeout(Duration::from_secs(3), events.next())
        .await
        .expect("first SSE event timed out")
        .expect("expected event")
        .expect("event should parse");
    assert_eq!(first.data, "first");
    assert!(
        timeout(Duration::from_secs(3), events.next())
            .await
            .expect("SSE completion timed out")
            .is_none()
    );
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
    assert_post_replay_requests(&captured);
}

#[tokio_test]
async fn test_sse_reconnect_replays_post_after_status_with_http_retry_disabled() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 503,
            headers: vec![],
            body: b"unavailable".to_vec(),
        },
        ResponsePlan::Immediate {
            status: 204,
            headers: vec![],
            body: Vec::new(),
        },
    ])
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = false;
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");
    let request = client
        .request(Method::POST, "/events")
        .bytes_body(b"subscription".as_slice())
        .disable_retry()
        .build();
    let reconnect = SseReconnectOptions {
        retry: RetryPolicy::builder()
            .max_attempts(2)
            .backoff(BackoffPolicy::immediate())
            .build()
            .expect("retry policy should be valid"),
        ..SseReconnectOptions::default()
    };
    let mut events = client.execute_sse_with_reconnect(request, reconnect);
    assert!(
        timeout(Duration::from_secs(3), events.next())
            .await
            .expect("SSE completion timed out")
            .is_none()
    );
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
    assert_post_replay_requests(&captured);
}

fn assert_post_replay_requests(captured: &[CapturedRequest]) {
    for request in captured {
        assert_eq!(request.method, "POST");
        assert_eq!(request.target, "/events");
        assert_eq!(request.body, b"subscription");
    }
}

#[tokio_test]
async fn test_sse_content_type_error_omits_sensitive_header_value() {
    const SECRET: &str = "content-type-review-secret";
    for native_sensitive in [true, false] {
        let server = spawn_one_shot_server(ResponsePlan::Immediate {
            status: 200,
            headers: vec![("Content-Type".to_owned(), format!("text/plain; token={SECRET}"))],
            body: Vec::new(),
        })
        .await;
        let mut options = HttpClientOptions::default();
        options.base_url = Some(server.base_url());
        if !native_sensitive {
            options.log_redaction_policy = qubit_redact::RedactionPolicy::default()
                .to_builder()
                .http(|http| {
                    let _ = http.header().raise("content-type", qubit_redact::Sensitivity::Secret);
                })
                .unwrap()
                .build()
                .unwrap();
        }
        let mut client = HttpClientBuilder::new().options(options).build().unwrap();
        if native_sensitive {
            client.add_response_interceptor(HttpResponseInterceptor::new(
                |context: &mut HttpResponseInterceptorContext| {
                    context
                        .headers_mut()
                        .get_mut("content-type")
                        .unwrap()
                        .set_sensitive(true);
                    Ok(())
                },
            ));
        }
        let request = client.request(Method::GET, "/sse-header-secret").build();
        let mut events = client.execute_sse_with_reconnect(request, SseReconnectOptions::default());
        let error = timeout(Duration::from_secs(3), events.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.kind(), HttpErrorKind::SseProtocol);
        assert_eq!(error.method(), Some(&Method::GET));
        assert_eq!(error.status(), Some(StatusCode::OK));
        assert_eq!(error.url().unwrap().path(), "/sse-header-secret");
        assert_eq!(
            error.message(),
            "Expected Content-Type 'text/event-stream' for SSE response"
        );
        for output in [error.message().to_owned(), error.to_string(), format!("{error:?}")] {
            assert!(!output.contains(SECRET), "sensitive Content-Type leaked");
        }
        assert!(events.next().await.is_none());
        let captured = timeout(Duration::from_secs(3), server.finish()).await.unwrap();
        assert_eq!(captured.target, "/sse-header-secret");
    }
}

#[tokio_test]
async fn test_sse_nonempty_cursor_wins_after_request_interceptors() {
    for mutation in 0..3 {
        let server = spawn_multi_shot_server(vec![
            ResponsePlan::Immediate {
                status: 200,
                headers: vec![("Content-Type".to_owned(), "text/event-stream".to_owned())],
                body: b"id: fresh\ndata: first\n\n".to_vec(),
            },
            ResponsePlan::Immediate {
                status: 200,
                headers: vec![("Content-Type".to_owned(), "text/event-stream".to_owned())],
                body: b"data: second\n\n".to_vec(),
            },
        ])
        .await;
        let mut options = HttpClientOptions::default();
        options.base_url = Some(server.base_url());
        options.add_header("last-event-id", "default-old").unwrap();
        let mut client = HttpClientBuilder::new().options(options).build().unwrap();
        client.add_header_injector(HttpHeaderInjector::new(|headers: &mut http::HeaderMap| {
            headers.insert("last-event-id", "sync-old".parse().unwrap());
            Ok(())
        }));
        client.add_async_header_injector(AsyncHttpHeaderInjector::new(|headers: &mut http::HeaderMap| {
            Box::pin(async move {
                headers.insert("last-event-id", "async-old".parse().unwrap());
                Ok(())
            })
        }));
        client.add_request_interceptor(HttpRequestInterceptor::new(move |request: &mut HttpRequest| {
            match mutation {
                0 => {
                    request.set_header("last-event-id", "interceptor-old")?;
                }
                1 => {
                    request.clear_headers();
                }
                _ => {
                    request.remove_header(&HeaderName::from_static("last-event-id"));
                }
            }
            Ok(())
        }));
        let request = client
            .request(Method::GET, "/sse-cursor-authority")
            .header("last-event-id", "template-old")
            .unwrap()
            .build();
        let mut events = client.execute_sse_with_reconnect(
            request,
            SseReconnectOptions {
                retry: build_retry_options(1, BackoffPolicy::immediate()),
                ..SseReconnectOptions::default()
            },
        );
        for expected in ["first", "second"] {
            let message = timeout(Duration::from_secs(3), events.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(message.data, expected);
            assert_eq!(message.last_event_id.as_deref(), Some("fresh"));
        }
        assert!(events.next().await.is_none());
        let requests = timeout(Duration::from_secs(3), server.finish()).await.unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1].headers.get("last-event-id").map(String::as_str),
            Some("fresh"),
            "interceptor mutation mode {mutation}"
        );
    }
}

/// Builds retry options for SSE reconnect tests.
///
/// # Parameters
/// - `max_reconnects`: Maximum reconnect attempts after the initial attempt.
/// - `backoff`: Backoff policy for reconnect attempts.
///
/// # Returns
/// Retry options with `max_attempts = max_reconnects + 1`.
fn build_retry_options(max_reconnects: u32, backoff: BackoffPolicy) -> RetryPolicy {
    build_retry_policy(max_reconnects, None, backoff)
}

/// Builds retry options for SSE reconnect tests with max elapsed-time limit.
///
/// # Parameters
/// - `max_reconnects`: Maximum reconnect attempts after the initial attempt.
/// - `max_elapsed`: Maximum elapsed reconnect duration.
/// - `backoff`: Backoff policy for reconnect attempts.
///
/// # Returns
/// Retry options with `max_attempts = max_reconnects + 1`.
fn build_retry_options_with_max_elapsed(
    max_reconnects: u32,
    max_elapsed: Duration,
    backoff: BackoffPolicy,
) -> RetryPolicy {
    build_retry_policy(max_reconnects, Some(max_elapsed), backoff)
}

/// Builds a retry policy from the reconnect limit, optional time budget, and
/// backoff.
fn build_retry_policy(max_reconnects: u32, max_elapsed: Option<Duration>, backoff: BackoffPolicy) -> RetryPolicy {
    let mut builder = RetryPolicy::builder().max_attempts(max_reconnects + 1).backoff(backoff);
    if let Some(max_elapsed) = max_elapsed {
        builder = builder.total_time_budget(max_elapsed);
    }
    builder.build().unwrap()
}

async fn run_retry_after_sse_reconnect(
    retry_after: &str,
    template_honor_retry_after: bool,
    interceptor_honor_retry_after: bool,
) -> Duration {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 503,
            headers: vec![("Retry-After".to_string(), retry_after.to_string())],
            body: b"retry".to_vec(),
        },
        ResponsePlan::Immediate {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            body: b"data: connected\n\n".to_vec(),
        },
    ])
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |request: &mut HttpRequest| {
        request.set_retry_override(
            HttpRequestRetryOverride::default().with_honor_retry_after(interceptor_honor_retry_after),
        );
        Ok(())
    }));
    let request = client
        .request(Method::GET, "/sse-retry-after")
        .honor_retry_after(template_honor_retry_after)
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(1, BackoffPolicy::immediate().prefer_retry_after()),
            reconnect_on_eof: false,
            honor_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );
    let started = Instant::now();
    assert_eq!(events.next().await.unwrap().unwrap().data, "connected");
    assert!(events.next().await.is_none());
    let requests = timeout(Duration::from_secs(3), server.finish()).await.unwrap();
    assert_eq!(requests.len(), 2);
    started.elapsed()
}

async fn run_sse_reset_header_layers(first_body: &'static [u8], reset: bool) {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            body: first_body.to_vec(),
        },
        ResponsePlan::Immediate {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            body: b"data: after\n\n".to_vec(),
        },
    ])
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.add_header("last-event-id", "default-old").unwrap();
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client.add_header_injector(HttpHeaderInjector::new(|headers: &mut http::HeaderMap| {
        headers.insert("last-event-id", "sync-old".parse().unwrap());
        Ok(())
    }));
    client.add_async_header_injector(AsyncHttpHeaderInjector::new(|headers: &mut http::HeaderMap| {
        Box::pin(async move {
            headers.insert("last-event-id", "async-old".parse().unwrap());
            Ok(())
        })
    }));
    client.add_request_interceptor(HttpRequestInterceptor::new(|request: &mut HttpRequest| {
        request.set_header("last-event-id", "interceptor-old")?;
        Ok(())
    }));
    let request = client
        .request(Method::GET, "/sse-reset-layers")
        .header("last-event-id", "template-old")
        .unwrap()
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(1, BackoffPolicy::immediate()),
            reconnect_on_eof: true,
            ..SseReconnectOptions::default()
        },
    );
    let first = events.next().await.unwrap().unwrap();
    assert_eq!(
        first.data,
        if first_body.starts_with(b"id:") {
            "reset"
        } else {
            "first"
        }
    );
    assert_eq!(events.next().await.unwrap().unwrap().data, "after");
    assert!(events.next().await.is_none());
    let requests = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].headers.get("last-event-id").map(String::as_str),
        Some("interceptor-old")
    );
    if reset {
        assert!(!requests[1].headers.contains_key("last-event-id"));
    } else {
        assert_eq!(
            requests[1].headers.get("last-event-id").map(String::as_str),
            Some("interceptor-old")
        );
    }
}

#[tokio_test]
async fn test_sse_reset_omits_last_event_id_from_all_header_layers() {
    run_sse_reset_header_layers(b"id:\n\ndata: reset\n\n", true).await;
}

#[tokio_test]
async fn test_sse_template_last_event_id_survives_without_server_reset() {
    run_sse_reset_header_layers(b"data: first\n\n", false).await;
}

#[tokio_test]
async fn test_execute_sse_reconnect_honors_delta_seconds_after_interceptor_override() {
    let elapsed = run_retry_after_sse_reconnect("1", false, true).await;
    assert!(elapsed >= Duration::from_millis(650), "elapsed={elapsed:?}");
}

#[tokio_test]
async fn test_execute_sse_with_reconnect_stops_normally_on_204() {
    for headers in [
        vec![],
        vec![("Content-Type".to_string(), "text/event-stream".to_string())],
    ] {
        let server = spawn_one_shot_server(ResponsePlan::Immediate {
            status: 204,
            headers,
            body: vec![],
        })
        .await;
        let mut options = HttpClientOptions::default();
        options.base_url = Some(server.base_url());
        let mut client = HttpClientBuilder::new().options(options).build().unwrap();
        let attempts = Arc::new(AtomicUsize::new(0));
        let observed = attempts.clone();
        client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        let request = client.request(Method::GET, "/sse-stop").build();
        let mut events = client.execute_sse_with_reconnect(request, SseReconnectOptions::default());
        let item = timeout(Duration::from_secs(3), events.next()).await.unwrap();
        assert!(item.is_none(), "204 must terminate normally without an SSE error");
        let captured = timeout(Duration::from_secs(3), server.finish()).await.unwrap();
        assert_eq!(captured.target, "/sse-stop");
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
}

#[tokio_test]
async fn test_execute_sse_with_reconnect_stops_on_204_after_reconnect() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            body: b"data: once\n\n".to_vec(),
        },
        ResponsePlan::Immediate {
            status: 204,
            headers: vec![],
            body: vec![],
        },
    ])
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    let request = client.request(Method::GET, "/sse-stop-after-reconnect").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(2, BackoffPolicy::immediate()),
            reconnect_on_eof: true,
            ..SseReconnectOptions::default()
        },
    );
    assert_eq!(events.next().await.unwrap().unwrap().data, "once");
    assert!(events.next().await.is_none());
    let requests = timeout(Duration::from_secs(3), server.finish()).await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[tokio_test]
async fn test_execute_sse_reconnect_honors_http_date_after_interceptor_override() {
    let date = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(2));
    let elapsed = run_retry_after_sse_reconnect(&date, false, true).await;
    assert!(elapsed >= Duration::from_millis(650), "elapsed={elapsed:?}");
}

#[tokio_test]
async fn test_execute_sse_reconnect_ignores_invalid_and_past_http_dates() {
    let past = httpdate::fmt_http_date(SystemTime::now() - Duration::from_secs(2));
    for value in ["invalid-date", past.as_str()] {
        let elapsed = run_retry_after_sse_reconnect(value, false, true).await;
        assert!(elapsed < Duration::from_millis(650), "{value}: elapsed={elapsed:?}");
    }
}

#[tokio_test]
async fn test_execute_sse_reconnect_interceptor_can_disable_template_retry_after() {
    let elapsed = run_retry_after_sse_reconnect("1", true, false).await;
    assert!(elapsed < Duration::from_millis(650), "elapsed={elapsed:?}");
}

#[tokio_test]
async fn test_execute_sse_with_reconnect_propagates_and_resets_last_event_id() {
    let plan = |body: &'static [u8]| ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![ResponseChunk {
            delay: Duration::ZERO,
            bytes: body.to_vec(),
        }],
        finish: true,
    };
    let server = spawn_multi_shot_server(vec![
        plan(b"id: evt-1\ndata: first\n\n"),
        plan(b"data: second\n\n"),
        plan(b"id: evt-2\n\n"),
        plan(b"data: third\n\n"),
        plan(b"id:\n\n"),
        plan(b"data: fifth\n\n"),
        plan(b"id: evt-3\ndata: resumed\n\n"),
        plan(b"data: after-resume\n\n"),
    ])
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client.request(Method::GET, "/sse-reconnect").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(7, BackoffPolicy::fixed(Duration::from_millis(1))),
            reconnect_on_eof: true,
            honor_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );
    let expected = [
        ("first", Some("evt-1")),
        ("second", Some("evt-1")),
        ("third", Some("evt-2")),
        ("fifth", Some("")),
        ("resumed", Some("evt-3")),
        ("after-resume", Some("evt-3")),
    ];
    for (data, id) in expected {
        let message = events.next().await.unwrap().unwrap();
        assert_eq!(message.data, data);
        assert_eq!(message.last_event_id.as_deref(), id);
    }
    assert!(events.next().await.is_none());
    let requests = timeout(Duration::from_secs(3), server.finish()).await.unwrap();
    assert_eq!(requests.len(), 8);
    assert_eq!(
        requests[1].headers.get("last-event-id").map(String::as_str),
        Some("evt-1")
    );
    assert_eq!(
        requests[2].headers.get("last-event-id").map(String::as_str),
        Some("evt-1")
    );
    assert_eq!(
        requests[3].headers.get("last-event-id").map(String::as_str),
        Some("evt-2")
    );
    assert_eq!(
        requests[4].headers.get("last-event-id").map(String::as_str),
        Some("evt-2")
    );
    assert!(!requests[5].headers.contains_key("last-event-id"));
    assert!(!requests[6].headers.contains_key("last-event-id"));
    assert_eq!(
        requests[7].headers.get("last-event-id").map(String::as_str),
        Some("evt-3")
    );
}

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client.request(Method::GET, "/sse-retry-delay").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(1, BackoffPolicy::fixed(Duration::from_millis(1))),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );
    assert_eq!(events.next().await.unwrap().unwrap().data, "first");
    assert_eq!(events.next().await.unwrap().unwrap().data, "second");
    assert!(events.next().await.is_none());
    let requests = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(requests.len(), 2);
}

#[tokio_test]
async fn test_execute_sse_with_reconnect_preserves_server_retry_across_reconnects() {
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
                bytes: b"retry: +1\ndata: third\n\n".to_vec(),
            }],
            finish: false,
        },
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"retry: 40\ndata: fourth\n\n".to_vec(),
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client
        .request(Method::GET, "/sse-server-retry-once-then-backoff")
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                4,
                BackoffPolicy::exponential(Duration::from_millis(40), 2.0, Duration::from_millis(200))
                    .expect("test backoff should be valid"),
            ),
            reconnect_on_eof: true,
            honor_server_retry: true,
            apply_jitter_to_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );
    assert_eq!(events.next().await.unwrap().unwrap().data, "first");
    assert_eq!(events.next().await.unwrap().unwrap().data, "second");
    assert_eq!(events.next().await.unwrap().unwrap().data, "third");
    assert_eq!(events.next().await.unwrap().unwrap().data, "fourth");
    assert_eq!(events.next().await.unwrap().unwrap().data, "done");
    assert!(events.next().await.is_none());

    let requests = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(requests.len(), 5);
}

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client.request(Method::GET, "/sse-server-retry-cap").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(1, BackoffPolicy::fixed(Duration::from_millis(1))),
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
}

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client.request(Method::GET, "/sse-server-retry-derived-cap").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                1,
                BackoffPolicy::uniform(Duration::from_millis(20), Duration::from_millis(80))
                    .expect("test backoff should be valid"),
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
}

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client.request(Method::GET, "/sse-disable-server-retry-jitter").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                reconnect_count as u32,
                BackoffPolicy::fixed(Duration::from_millis(1))
                    .with_bounded_jitter(1.0)
                    .expect("test jitter should be valid"),
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
}

#[tokio_test]
async fn test_execute_sse_with_reconnect_respects_retry_max_elapsed() {
    let server = spawn_multi_shot_server(vec![ResponsePlan::Immediate {
        status: 500,
        headers: vec![],
        body: b"server-error".to_vec(),
    }])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client.request(Method::GET, "/sse-max-elapsed").build();
    // The proposed delay alone exhausts the budget, regardless of request latency.
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options_with_max_elapsed(
                5,
                Duration::from_secs(1),
                BackoffPolicy::fixed(Duration::from_secs(1)),
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
    assert_eq!(error.kind, HttpErrorKind::RetryBudgetExceeded);
    assert_eq!(error.status, Some(StatusCode::INTERNAL_SERVER_ERROR));
    assert!(error.source.is_some(), "last retryable error should be chained");
    assert!(
        error.message.contains("SSE reconnect max duration exceeded"),
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
    assert_eq!(captured.len(), 1);
}

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client
        .request(Method::GET, "/sse-max-elapsed-before-eof-reconnect")
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options_with_max_elapsed(
                5,
                Duration::from_millis(100),
                BackoffPolicy::fixed(Duration::from_millis(150)),
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
    assert_eq!(error.kind, HttpErrorKind::RetryBudgetExceeded);
    assert!(
        error.message.contains("SSE reconnect max duration exceeded"),
        "error message should mention max elapsed budget: {}",
        error.message
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse-max-elapsed-before-eof-reconnect");
}

#[tokio_test]
async fn test_sse_operation_budget_excludes_stream_consumption_for_eof_and_body_error() {
    for body_failure in [false, true] {
        let server = spawn_budget_lifecycle_server(body_failure).await;
        let mut options = HttpClientOptions::default();
        options.base_url = Some(server.base_url.clone());
        options.timeouts.read_timeout = Duration::from_secs(4);
        options.timeouts.request_timeout = Some(Duration::from_secs(4));
        options.timeouts.response_header_timeout = Duration::from_secs(4);
        let client = HttpClientBuilder::new().options(options).build().unwrap();
        let request = client.request(Method::GET, "/operation-budget-lifecycle").build();
        let retry = RetryPolicy::builder()
            .max_attempts(2)
            .operation_time_budget(Duration::from_millis(250))
            .backoff(BackoffPolicy::immediate())
            .build()
            .unwrap();
        let mut events = client.execute_sse_with_reconnect(
            request,
            SseReconnectOptions {
                retry,
                reconnect_on_eof: true,
                honor_server_retry: false,
                ..SseReconnectOptions::default()
            },
        );

        let first = timeout(Duration::from_secs(3), events.next())
            .await
            .expect("first event timed out")
            .expect("first connection should emit an event")
            .expect("first event should decode");
        assert_eq!(first.data, "first");
        server
            .first_body_sent_rx
            .await
            .expect("server should signal after sending the first event");
        tokio::time::sleep(Duration::from_millis(420)).await;
        let _ = server.release_first_tx.send(());

        let still_alive = timeout(Duration::from_secs(3), events.next())
            .await
            .expect("second event from the active connection timed out")
            .expect("active connection should emit after operation time expires")
            .expect("active connection event should decode");
        assert_eq!(still_alive.data, "still-alive");

        let second = timeout(Duration::from_secs(3), events.next())
            .await
            .expect("second event timed out")
            .expect("operation-only budget should admit a second connection")
            .expect("second event should decode");
        assert_eq!(second.data, "second");
        assert!(
            timeout(Duration::from_secs(3), events.next())
                .await
                .expect("reconnect stream completion timed out")
                .is_none()
        );
        assert_eq!(server.accepted.load(Ordering::SeqCst), 2);
        server.join_handle.abort();
    }
}

#[tokio_test]
async fn test_sse_total_budget_counts_live_stream_and_paused_consumer_without_interrupting() {
    let server = spawn_budget_lifecycle_server(false).await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url.clone());
    options.timeouts.read_timeout = Duration::from_secs(4);
    options.timeouts.request_timeout = Some(Duration::from_secs(4));
    options.timeouts.response_header_timeout = Duration::from_secs(4);
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client.request(Method::GET, "/total-budget-lifecycle").build();
    let retry = RetryPolicy::builder()
        .max_attempts(2)
        .total_time_budget(Duration::from_millis(180))
        .backoff(BackoffPolicy::immediate())
        .build()
        .unwrap();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry,
            reconnect_on_eof: true,
            honor_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );

    let first = timeout(Duration::from_secs(3), events.next())
        .await
        .expect("first event timed out")
        .expect("first connection should emit an event")
        .expect("first event should decode");
    assert_eq!(first.data, "first");
    server
        .first_body_sent_rx
        .await
        .expect("server should signal after sending the first event");
    // Pausing polling keeps the stream active. Total elapsed passes, but the
    // admission budget must not cancel the currently open response.
    tokio::time::sleep(Duration::from_millis(420)).await;
    assert!(timeout(Duration::from_millis(80), events.next()).await.is_err());
    assert_eq!(server.accepted.load(Ordering::SeqCst), 1);
    let _ = server.release_first_tx.send(());

    let still_alive = timeout(Duration::from_secs(3), events.next())
        .await
        .expect("active stream event timed out after total time expired")
        .expect("active stream should emit after total time expires")
        .expect("active stream event should decode");
    assert_eq!(still_alive.data, "still-alive");

    let error = timeout(Duration::from_secs(3), events.next())
        .await
        .expect("budget error timed out")
        .expect("total budget exhaustion should yield an error")
        .expect_err("total budget should reject the reconnect after EOF");
    assert_eq!(error.kind, HttpErrorKind::RetryBudgetExceeded);
    assert!(error.message.contains("SSE reconnect max duration exceeded"));
    assert_eq!(server.accepted.load(Ordering::SeqCst), 1);
    server.join_handle.abort();
}

struct BudgetLifecycleServer {
    base_url: Url,
    first_body_sent_rx: oneshot::Receiver<()>,
    release_first_tx: oneshot::Sender<()>,
    accepted: Arc<AtomicUsize>,
    join_handle: JoinHandle<()>,
}

async fn spawn_budget_lifecycle_server(body_failure: bool) -> BudgetLifecycleServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = Url::parse(&format!("http://{addr}/")).unwrap();
    let (first_body_sent_tx, first_body_sent_rx) = oneshot::channel();
    let (release_first_tx, release_first_rx) = oneshot::channel();
    let accepted = Arc::new(AtomicUsize::new(0));
    let accepted_in_task = accepted.clone();
    let join_handle = tokio::spawn(async move {
        let mut first_body_sent_tx = Some(first_body_sent_tx);
        let mut release_first_rx = Some(release_first_rx);
        for attempt in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            accepted_in_task.fetch_add(1, Ordering::SeqCst);
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                if stream.read_exact(&mut byte).await.is_err() {
                    return;
                }
                request.push(byte[0]);
            }
            let is_first = attempt == 0;
            let failure = is_first && body_failure;
            let headers = if failure {
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n".as_slice()
            } else {
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".as_slice()
            };
            stream.write_all(headers).await.unwrap();
            let first_event = if is_first {
                b"data: first\n\n".as_slice()
            } else {
                b"data: second\n\n".as_slice()
            };
            if failure {
                stream.write_all(first_event).await.unwrap();
            } else {
                let frame = format!("{:X}\r\n", first_event.len());
                stream.write_all(frame.as_bytes()).await.unwrap();
                stream.write_all(first_event).await.unwrap();
                stream.write_all(b"\r\n").await.unwrap();
            }
            stream.flush().await.unwrap();
            if is_first {
                let _ = first_body_sent_tx.take().expect("first notification sender").send(());
                let _ = release_first_rx.take().expect("first release receiver").await;
                let alive_event = b"data: still-alive\n\n";
                if failure {
                    stream.write_all(alive_event).await.unwrap();
                } else {
                    let frame = format!("{:X}\r\n", alive_event.len());
                    stream.write_all(frame.as_bytes()).await.unwrap();
                    stream.write_all(alive_event).await.unwrap();
                    stream.write_all(b"\r\n").await.unwrap();
                }
                stream.flush().await.unwrap();
            }
            if failure {
                let _ = stream.shutdown().await;
            } else {
                stream.write_all(b"0\r\n\r\n").await.unwrap();
                let _ = stream.shutdown().await;
            }
        }
    });
    BudgetLifecycleServer {
        base_url,
        first_body_sent_rx,
        release_first_tx,
        accepted,
        join_handle,
    }
}

#[tokio_test]
async fn test_execute_sse_with_reconnect_sleep_can_be_cancelled() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![ResponseChunk {
            delay: Duration::ZERO,
            bytes: b"data: connected\n\n".to_vec(),
        }],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));

    let token = HttpCancellationToken::new();

    let request = client
        .request(Method::GET, "/sse-cancel-reconnect-sleep")
        .cancellation_token(token.clone())
        .build();

    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(1, BackoffPolicy::fixed(Duration::from_secs(1))),
            reconnect_on_eof: true,
            honor_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );
    let first = events
        .next()
        .await
        .expect("initial SSE connection should emit one event")
        .expect("initial SSE event should decode");
    assert_eq!(first.data, "connected");

    let next = events.next();
    pin!(next);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(
        next.as_mut().poll(&mut context).is_pending(),
        "EOF should enter reconnect backoff before yielding another item"
    );
    token.cancel();
    let error = match next.as_mut().poll(&mut context) {
        Poll::Ready(Some(Err(error))) => error,
        Poll::Ready(Some(Ok(_))) => {
            panic!("cancelled reconnect must not yield another event")
        }
        Poll::Ready(None) => panic!("cancelled reconnect must yield an error"),
        Poll::Pending => panic!("ready cancellation must interrupt backoff"),
    };
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "A cancellation must stop the next attempt"
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse-cancel-reconnect-sleep");
}

#[tokio_test]
async fn test_execute_sse_with_reconnect_disables_inner_http_retry() {
    let mut options = HttpClientOptions::default();
    options
        .set_base_url("http://127.0.0.1:18080")
        .expect("base URL should parse");
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();

    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Err(HttpError::transport("injector transient transport failure"))
    }));

    let request = client.request(Method::GET, "/sse-disable-inner-retry").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(1, BackoffPolicy::fixed(Duration::from_millis(1))),
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

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client.request(Method::GET, "/sse-content-type-check").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(3, BackoffPolicy::fixed(Duration::from_millis(1))),
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

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client.request(Method::GET, "/sse-missing-content-type").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(3, BackoffPolicy::fixed(Duration::from_millis(1))),
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

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client.request(Method::GET, "/sse-non-utf8-content-type").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(3, BackoffPolicy::fixed(Duration::from_millis(1))),
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
    assert!(
        error
            .url
            .as_ref()
            .is_some_and(|url| url.path() == "/sse-non-utf8-content-type")
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse-non-utf8-content-type");
}

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client
        .request(Method::GET, "/sse-content-type-prefix-collision")
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(3, BackoffPolicy::fixed(Duration::from_millis(1))),
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

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client.request(Method::GET, "/sse-custom-backoff").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(
                2,
                BackoffPolicy::exponential(Duration::from_millis(80), 3.0, Duration::from_millis(200))
                    .expect("test backoff should be valid"),
            ),
            reconnect_on_eof: true,
            honor_server_retry: false,
            ..SseReconnectOptions::default()
        },
    );
    assert_eq!(events.next().await.unwrap().unwrap().data, "done");
    assert!(events.next().await.is_none());
    let requests = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(requests.len(), 3);
}

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }));

    let request = client.request(Method::GET, "/sse-protocol-error").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(3, BackoffPolicy::fixed(Duration::from_millis(1))),
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

#[tokio_test]
async fn test_execute_sse_with_reconnect_does_not_retry_nonretryable_eof_errors() {
    let mut cases = vec![
        (HttpErrorKind::SseProtocol, None),
        (HttpErrorKind::SseDecode, None),
        (HttpErrorKind::Decode, None),
        (HttpErrorKind::Cancelled, None),
    ];
    cases.extend(
        [
            StatusCode::BAD_REQUEST,
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::NOT_FOUND,
        ]
        .into_iter()
        .map(|status| (HttpErrorKind::Status, Some(status))),
    );
    cases.push((HttpErrorKind::Status, None));

    for (kind, expected_status) in cases {
        for source_mode in 0..3 {
            let mut options = HttpClientOptions::default();
            options
                .set_base_url("http://127.0.0.1:18080")
                .expect("base URL should parse");
            let mut client = HttpClientBuilder::new().options(options).build().unwrap();
            let attempts = Arc::new(AtomicUsize::new(0));
            let attempts_for_interceptor = Arc::clone(&attempts);
            client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
                attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
                let mut error = match source_mode {
                    0 => HttpError::new(kind, "unexpected eof in application protocol"),
                    1 => HttpError::new(kind, "application protocol failure")
                        .with_source(IoError::other("unexpected eof from source")),
                    2 => HttpError::new(kind, "application protocol failure")
                        .with_source(IoError::new(std::io::ErrorKind::UnexpectedEof, "stream ended")),
                    _ => unreachable!("test matrix only defines three source modes"),
                };
                if let Some(status) = expected_status {
                    error = error.with_status(status);
                }
                Err(error)
            }));

            let request = client.request(Method::GET, "/sse-nonretryable-eof").build();
            let mut events = client.execute_sse_with_reconnect(
                request,
                SseReconnectOptions {
                    retry: build_retry_options(1, BackoffPolicy::fixed(Duration::from_millis(1))),
                    reconnect_on_eof: false,
                    honor_server_retry: false,
                    ..SseReconnectOptions::default()
                },
            );
            let error = timeout(Duration::from_secs(3), events.next())
                .await
                .expect("stream should return promptly")
                .expect("one error item")
                .expect_err("nonretryable error must be returned");
            assert_eq!(error.kind, kind);
            assert_eq!(error.status, expected_status);
            match source_mode {
                0 => assert!(error.source.is_none()),
                1 => assert_eq!(
                    error.source.as_ref().map(ToString::to_string).as_deref(),
                    Some("unexpected eof from source"),
                ),
                2 => assert_eq!(
                    error.source.as_ref().map(ToString::to_string).as_deref(),
                    Some("stream ended")
                ),
                _ => unreachable!("test matrix only defines three source modes"),
            }
            assert_eq!(
                attempts.load(Ordering::Relaxed),
                1,
                "kind={kind:?}, source_mode={source_mode}"
            );
            assert!(events.next().await.is_none());
        }
    }
}

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client
        .request(Method::GET, "/sse-invalid-last-event-id")
        .query_param("access_token", "http001-query-secret")
        .build();
    let expected_url = request.resolved_url().unwrap();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(1, BackoffPolicy::fixed(Duration::from_millis(1))),
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
    assert!(!error.message.contains("bad"));
    assert_eq!(error.method(), Some(&Method::GET));
    assert_eq!(error.url(), Some(&expected_url));
    assert_eq!(error.status(), None);
    assert!(!format!("{error}").contains("http001-query-secret"));
    assert!(!format!("{error:?}").contains("http001-query-secret"));
    assert!(events.next().await.is_none());

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(
        captured.target,
        "/sse-invalid-last-event-id?access_token=http001-query-secret"
    );
}

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();

    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        let current = attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        if current == 0 {
            Err(HttpError::other("unexpected eof while preparing local SSE pipeline"))
        } else {
            Ok(())
        }
    }));

    let request = client.request(Method::GET, "/sse-unexpected-eof").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(1, BackoffPolicy::fixed(Duration::from_millis(1))),
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

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();

    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        let current = attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        if current == 0 {
            Err(HttpError::other("local SSE source failure")
                .with_source(IoError::other("unexpected eof from wrapped source")))
        } else {
            Ok(())
        }
    }));

    let request = client.request(Method::GET, "/sse-unexpected-eof-source").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(1, BackoffPolicy::fixed(Duration::from_millis(1))),
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

#[tokio_test]
async fn test_execute_sse_with_reconnect_does_not_retry_non_eof_source_error() {
    let mut options = HttpClientOptions::default();
    options
        .set_base_url("http://127.0.0.1:18080")
        .expect("base URL should parse");
    options.timeouts.read_timeout = Duration::from_secs(1);
    options.timeouts.response_header_timeout = Duration::from_secs(1);
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();

    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Err(HttpError::other("local SSE non-EOF failure").with_source(IoError::other("ordinary source error")))
    }));

    let request = client.request(Method::GET, "/sse-non-eof-source").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(3, BackoffPolicy::fixed(Duration::from_millis(1))),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
}

#[tokio_test]
async fn test_retry_control_applies_after_unterminated_retry_line_at_eof() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"retry: 100".to_vec(),
            }],
            finish: false,
        },
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"data: recovered\n\n".to_vec(),
            }],
            finish: true,
        },
    ])
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    let request = client.request(Method::GET, "/retry-control-eof-fragment").build();
    let started = Instant::now();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(1, BackoffPolicy::fixed(Duration::from_millis(100))),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );
    let message = timeout(Duration::from_secs(3), events.next())
        .await
        .expect("retry reconnect timed out")
        .expect("second response should dispatch")
        .expect("second response should be valid");
    assert_eq!(message.data, "recovered");
    assert!(started.elapsed() >= Duration::from_millis(100));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
}

#[tokio_test]
async fn test_retry_control_timeout_reconnect_honors_complete_retry_line() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        total_length: 1024,
        prefix: b"retry: 1000\n".to_vec(),
        delay: Duration::from_millis(400),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_millis(80);
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let attempts = Arc::new(Mutex::new(Vec::new()));
    let observed = attempts.clone();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        observed.lock().unwrap().push(Instant::now());
        Ok(())
    }));
    let request = client.request(Method::GET, "/retry-control-read-timeout").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(1, BackoffPolicy::immediate()),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );
    let error = timeout(Duration::from_secs(2), events.next())
        .await
        .expect("read timeout and retry should terminate promptly")
        .expect("read timeout should yield a terminal error")
        .expect_err("failed reconnect should yield an error");
    assert!(matches!(
        error.kind,
        HttpErrorKind::Transport | HttpErrorKind::ReadTimeout
    ));
    {
        let attempts = attempts.lock().unwrap();
        assert_eq!(attempts.len(), 2);
        assert!(
            attempts[1].duration_since(attempts[0]) >= Duration::from_millis(1_000),
            "complete retry line should control reconnect delay after a body-read timeout"
        );
    }
    let captured = timeout(Duration::from_secs(2), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/retry-control-read-timeout");
}

#[tokio_test]
async fn test_execute_sse_with_reconnect_does_not_retry_cancelled_error() {
    let mut options = HttpClientOptions::default();
    options
        .set_base_url("http://127.0.0.1:18080")
        .expect("base URL should parse");
    options.timeouts.read_timeout = Duration::from_secs(1);
    options.timeouts.response_header_timeout = Duration::from_secs(1);
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();

    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Err(HttpError::cancelled("cancelled before SSE send"))
    }));

    let request = client.request(Method::GET, "/sse-cancelled").build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(3, BackoffPolicy::fixed(Duration::from_millis(1))),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );

    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
}

#[tokio_test]
async fn test_execute_sse_with_reconnect_reports_cancelled_stream_before_reading_body() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![ResponseChunk {
            delay: Duration::from_millis(0),
            bytes: b"data: should-not-be-read\n\n".to_vec(),
        }],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(1);
    options.timeouts.response_header_timeout = Duration::from_secs(1);
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));

    let token = HttpCancellationToken::new();
    let token_for_interceptor = token.clone();
    client.add_response_interceptor(HttpResponseInterceptor::new(
        move |_meta: &mut HttpResponseInterceptorContext| {
            token_for_interceptor.cancel();
            Ok(())
        },
    ));

    let request = client
        .request(Method::GET, "/sse-cancel-before-body")
        .cancellation_token(token)
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(3, BackoffPolicy::fixed(Duration::from_millis(1))),
            reconnect_on_eof: true,
            honor_server_retry: true,
            ..SseReconnectOptions::default()
        },
    );

    let error = events
        .next()
        .await
        .expect("cancelled SSE stream should emit one item")
        .expect_err("cancelled SSE stream should fail");
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(
        error.message.contains("cancelled"),
        "error should explain cancellation: {error:?}"
    );
    assert!(events.next().await.is_none());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "unchanged A cancellation must not admit a reconnect"
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/sse-cancel-before-body");
}

#[tokio_test]
async fn test_sse_reconnect_replacement_token_cancels_current_body_read() {
    let mut server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![
            ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"data:".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_millis(250),
                bytes: b" not-read\n\n".to_vec(),
            },
        ],
        finish: true,
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let token_a = HttpCancellationToken::new();
    let token_b = HttpCancellationToken::new();
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    let replacement = token_b.clone();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |request: &mut HttpRequest| {
        observed.fetch_add(1, Ordering::SeqCst);
        request.set_cancellation_token(replacement.clone());
        Ok(())
    }));
    let request = client
        .request(Method::GET, "/replace-token-b")
        .cancellation_token(token_a)
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(2, BackoffPolicy::immediate()),
            reconnect_on_eof: true,
            ..SseReconnectOptions::default()
        },
    );
    let next = events.next();
    tokio::pin!(next);
    tokio::select! {
        _ = server.wait_until_response_started() => {},
        item = &mut next => panic!("stream ended before response body became readable: {item:?}"),
    }
    token_b.cancel();
    let error = next.await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "B cancellation must not admit another request"
    );
    let _ = server.finish().await;
}

#[tokio_test]
async fn test_sse_reconnect_replacement_keeps_original_token_for_reconnect_wait() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![ResponseChunk {
            delay: Duration::ZERO,
            bytes: b"data: delivered\n\n".to_vec(),
        }],
        finish: true,
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let token_a = HttpCancellationToken::new();
    let token_b = HttpCancellationToken::new();
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    let replacement = token_b.clone();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |request: &mut HttpRequest| {
        observed.fetch_add(1, Ordering::SeqCst);
        request.set_cancellation_token(replacement.clone());
        Ok(())
    }));
    let cancel_flow = token_a.clone();
    client.add_response_interceptor(HttpResponseInterceptor::new(
        move |_meta: &mut HttpResponseInterceptorContext| {
            cancel_flow.cancel();
            Ok(())
        },
    ));
    let request = client
        .request(Method::GET, "/flow-token-wait")
        .cancellation_token(token_a)
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(2, BackoffPolicy::fixed(Duration::from_secs(10))),
            reconnect_on_eof: true,
            ..SseReconnectOptions::default()
        },
    );
    assert_eq!(events.next().await.unwrap().unwrap().data, "delivered");
    let error = timeout(Duration::from_secs(3), events.next())
        .await
        .expect("flow cancellation should interrupt reconnect wait")
        .expect("flow cancellation should yield an error")
        .expect_err("cancelled flow must not yield another message");
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "flow cancellation must prevent the next request"
    );
    let _ = server.finish().await;
}

#[tokio_test]
async fn test_sse_reconnect_removing_attempt_token_preserves_flow_cancellation() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![ResponseChunk {
            delay: Duration::ZERO,
            bytes: b"data: delivered\n\n".to_vec(),
        }],
        finish: true,
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let token_a = HttpCancellationToken::new();
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    let clear_token = HttpRequestInterceptor::new(move |request: &mut HttpRequest| {
        observed.fetch_add(1, Ordering::SeqCst);
        request.clear_cancellation_token();
        Ok(())
    });
    client.add_request_interceptor(clear_token);
    let cancel_flow = token_a.clone();
    client.add_response_interceptor(HttpResponseInterceptor::new(
        move |_meta: &mut HttpResponseInterceptorContext| {
            cancel_flow.cancel();
            Ok(())
        },
    ));
    let request = client
        .request(Method::GET, "/remove-attempt-token")
        .cancellation_token(token_a)
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(2, BackoffPolicy::fixed(Duration::from_secs(10))),
            reconnect_on_eof: true,
            ..SseReconnectOptions::default()
        },
    );
    assert_eq!(events.next().await.unwrap().unwrap().data, "delivered");
    let error = timeout(Duration::from_secs(3), events.next())
        .await
        .expect("flow token should stop reconnect wait")
        .expect("flow cancellation should yield an error")
        .expect_err("cancelled flow must not yield another message");
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "flow cancellation must prevent the next request"
    );
    let _ = server.finish().await;
}

#[tokio_test]
async fn test_sse_reconnect_original_token_cancels_after_interceptor_before_send() {
    let mut options = HttpClientOptions::default();
    options.set_base_url("http://127.0.0.1:9").unwrap();
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let token_a = HttpCancellationToken::new();
    let cancel = token_a.clone();
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        observed.fetch_add(1, Ordering::SeqCst);
        cancel.cancel();
        Ok(())
    }));
    let request = client
        .request(Method::GET, "/cancel-before-send")
        .cancellation_token(token_a)
        .build();
    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            retry: build_retry_options(3, BackoffPolicy::immediate()),
            reconnect_on_eof: true,
            ..SseReconnectOptions::default()
        },
    );
    let error = events.next().await.unwrap().unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "A cancellation before send must prevent another attempt"
    );
    assert!(events.next().await.is_none());
}
