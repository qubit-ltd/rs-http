// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream;
use http::HeaderMap;
use http::Method;
use http::StatusCode;
use qubit_http::AsyncHttpHeaderInjector;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use qubit_http::HttpRequestBodyByteStream;
use qubit_http::RetryHint;
use qubit_retry::BackoffPolicy;
use tokio::sync::oneshot;
use tokio::test as tokio_test;
use tokio::time::Instant as TokioInstant;
use tokio::time::sleep;
use tokio::time::timeout;

use crate::common::ResponseChunk;
use crate::common::ResponsePlan;
use crate::common::spawn_multi_shot_server;
use crate::common::spawn_one_shot_server;

fn spawn_test_watchdog(timeout: Duration) -> (mpsc::Sender<()>, oneshot::Receiver<()>, thread::JoinHandle<()>) {
    let (done_tx, done_rx) = mpsc::channel();
    let (expired_tx, expired_rx) = oneshot::channel();
    let watchdog_thread = thread::spawn(move || {
        if done_rx.recv_timeout(timeout).is_err() {
            let _ = expired_tx.send(());
        }
    });
    (done_tx, expired_rx, watchdog_thread)
}

#[tokio_test(flavor = "current_thread")]
async fn test_retry_max_duration_watchdog_signals_and_cleans_up_tasks() {
    let (_watchdog_done_tx, mut watchdog_expired_rx, watchdog_thread) = spawn_test_watchdog(Duration::from_millis(10));
    let keepalive = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    let controller = tokio::spawn(std::future::pending::<()>());

    timeout(Duration::from_secs(2), &mut watchdog_expired_rx)
        .await
        .expect("watchdog signal should arrive before the test timeout")
        .expect("watchdog thread should report expiration");

    watchdog_thread.join().expect("expired watchdog thread should exit");
    keepalive.abort();
    assert!(
        keepalive
            .await
            .expect_err("keepalive should be cancelled")
            .is_cancelled()
    );
    controller.abort();
    assert!(
        controller
            .await
            .expect_err("controller should be cancelled")
            .is_cancelled()
    );
}

#[tokio_test]
async fn test_async_header_injector_preparation_is_outside_request_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.request_timeout = Some(Duration::from_millis(100));
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client.add_async_header_injector(AsyncHttpHeaderInjector::new(|_headers: &mut HeaderMap| {
        Box::pin(async move {
            sleep(Duration::from_millis(300)).await;
            Ok(())
        })
    }));

    let request = client.request(Method::GET, "/slow-header-preparation").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("preparation should not consume request_timeout");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.bytes().await.unwrap(), b"ok".as_slice());
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/slow-header-preparation");
}

#[tokio_test]
async fn test_streaming_body_preparation_is_outside_request_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.request_timeout = Some(Duration::from_millis(100));
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let factory = || -> Pin<Box<dyn Future<Output = HttpRequestBodyByteStream> + Send>> {
        Box::pin(async move {
            sleep(Duration::from_millis(300)).await;
            Box::pin(stream::iter([Ok(Bytes::from_static(b"payload"))])) as HttpRequestBodyByteStream
        })
    };
    let request = client
        .request(Method::POST, "/slow-body-preparation")
        .streaming_body(factory)
        .build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("preparation should not consume request_timeout");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.bytes().await.unwrap(), b"ok".as_slice());
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.body, b"payload");
}

#[tokio_test]
async fn test_async_header_preparation_uses_response_header_timeout() {
    let server = spawn_multi_shot_server(vec![]).await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.request_timeout = Some(Duration::from_secs(2));
    options.timeouts.response_header_timeout = Duration::from_millis(50);
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client.add_async_header_injector(AsyncHttpHeaderInjector::new(|_headers: &mut HeaderMap| {
        Box::pin(async move {
            sleep(Duration::from_millis(300)).await;
            Ok(())
        })
    }));

    let request = client.request(Method::GET, "/header-preparation-timeout").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("slow preparation should hit response_header_timeout");
    assert_eq!(error.kind, HttpErrorKind::ResponseHeaderTimeout);
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("empty server should finish without a request");
    assert!(captured.is_empty());
}

#[tokio_test]
async fn test_streaming_body_preparation_uses_response_header_timeout() {
    let server = spawn_multi_shot_server(vec![]).await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.request_timeout = Some(Duration::from_secs(2));
    options.timeouts.response_header_timeout = Duration::from_millis(50);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let factory = || -> Pin<Box<dyn Future<Output = HttpRequestBodyByteStream> + Send>> {
        Box::pin(async move {
            sleep(Duration::from_millis(300)).await;
            Box::pin(stream::iter([Ok(Bytes::from_static(b"payload"))])) as HttpRequestBodyByteStream
        })
    };
    let request = client
        .request(Method::POST, "/body-preparation-timeout")
        .streaming_body(factory)
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("slow preparation should hit response_header_timeout");
    assert_eq!(error.kind, HttpErrorKind::ResponseHeaderTimeout);
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("empty server should finish without a request");
    assert!(captured.is_empty());
}

#[test]
fn test_client_builder_rejects_unrepresentable_response_header_deadline() {
    let mut options = HttpClientOptions::default();
    options.timeouts.response_header_timeout = Duration::MAX;

    let error = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect_err("unrepresentable response-header deadline should be rejected");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("response_header_timeout"));
}

#[tokio_test(flavor = "current_thread", start_paused = true)]
async fn test_retry_max_duration_allows_admitted_request_to_finish_after_budget() {
    const WATCHDOG_TIMEOUT: Duration = Duration::from_secs(15);
    let max_duration = Duration::from_millis(500);

    let (watchdog_done_tx, mut watchdog_expired_rx, watchdog_thread) = spawn_test_watchdog(WATCHDOG_TIMEOUT);

    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 503,
            headers: vec![],
            body: b"retryable failure".to_vec(),
        },
        ResponsePlan::Immediate {
            status: 200,
            headers: vec![],
            body: b"success".to_vec(),
        },
    ])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.request_timeout = Some(Duration::from_secs(30));
    options.timeouts.response_header_timeout = Duration::from_secs(30);
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.max_duration = Some(max_duration);
    options.retry.backoff = BackoffPolicy::fixed(Duration::ZERO);
    let mut client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let attempt_count = Arc::new(AtomicUsize::new(0));
    let (admitted_tx, admitted_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let admitted_tx = Arc::new(Mutex::new(Some(admitted_tx)));
    let release_rx = Arc::new(Mutex::new(Some(release_rx)));
    client.add_async_header_injector(AsyncHttpHeaderInjector::new({
        let attempt_count = Arc::clone(&attempt_count);
        let admitted_tx = Arc::clone(&admitted_tx);
        let release_rx = Arc::clone(&release_rx);
        move |_headers: &mut HeaderMap| {
            let attempt = attempt_count.fetch_add(1, Ordering::SeqCst);
            if attempt == 1 {
                let admitted_tx = admitted_tx
                    .lock()
                    .expect("admission signal mutex poisoned")
                    .take()
                    .expect("second attempt admission signal should be available");
                let release_rx = release_rx
                    .lock()
                    .expect("release signal mutex poisoned")
                    .take()
                    .expect("second attempt release signal should be available");
                Box::pin(async move {
                    admitted_tx
                        .send(())
                        .expect("test controller should wait for second attempt");
                    release_rx.await.expect("test controller should release second attempt");
                    Ok(())
                })
            } else {
                Box::pin(async { Ok(()) })
            }
        }
    }));

    let controller = tokio::spawn(async move {
        admitted_rx
            .await
            .expect("second attempt should reach the async header injector");
        tokio::time::advance(Duration::from_millis(700)).await;
        release_tx
            .send(())
            .expect("second attempt should still be waiting for release");
    });
    let keepalive = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });

    let started_at = TokioInstant::now();
    let request = client.request(Method::GET, "/admitted-after-budget").build();
    let run_result = tokio::select! {
        result = async {
            let mut response = client.execute(request).await.map_err(|_| "HTTP execution failed")?;
            let status = response.status();
            let body = response.bytes().await.map_err(|_| "reading response body failed")?;
            let captured = server.finish().await;
            Ok::<_, &'static str>((status, body.to_vec(), captured))
        } => result,
        _ = &mut watchdog_expired_rx => Err("real-time watchdog expired"),
    };

    keepalive.abort();
    let keepalive_result = keepalive.await;
    let controller_result = if run_result.is_ok() {
        controller.await
    } else {
        controller.abort();
        controller.await
    };
    let _ = watchdog_done_tx.send(());
    watchdog_thread.join().expect("real-time watchdog thread should exit");

    assert!(
        keepalive_result
            .expect_err("keepalive should be cancelled")
            .is_cancelled()
    );
    if run_result.is_ok() {
        controller_result.expect("admission controller should complete successfully");
    } else {
        assert!(
            controller_result
                .expect_err("admission controller should be cancelled")
                .is_cancelled()
        );
    }
    let (status, body, captured) = run_result.expect("test flow should complete before watchdog");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, b"success");
    assert!(
        TokioInstant::now().duration_since(started_at) >= Duration::from_millis(700),
        "virtual elapsed time should pass the retry budget"
    );
    assert_eq!(captured.len(), 2);
    assert!(captured.iter().all(|request| request.method == "GET"));
    assert!(
        captured
            .iter()
            .all(|request| request.target == "/admitted-after-budget")
    );
}

#[tokio_test]
async fn test_client_level_request_timeout_triggers_timeout_classification() {
    let server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_millis(250),
        status: 200,
        headers: vec![],
        body: b"late".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_millis(80));

    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client.request(Method::GET, "/request-timeout").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::RequestTimeout);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}

#[tokio_test]
async fn test_request_level_timeout_overrides_client_level_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_millis(250),
        status: 200,
        headers: vec![],
        body: b"late".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(5));

    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client
        .request(Method::GET, "/request-timeout-override")
        .request_timeout(Duration::from_millis(80))
        .expect("positive request timeout should be accepted")
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::RequestTimeout);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}

#[tokio_test]
async fn test_client_level_response_header_timeout_triggers_response_header_timeout_error() {
    let server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_millis(250),
        status: 200,
        headers: vec![],
        body: b"late".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.response_header_timeout = Duration::from_millis(80);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(5));

    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client.request(Method::GET, "/client-write-timeout").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::ResponseHeaderTimeout);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}

#[tokio_test]
async fn test_request_level_response_header_timeout_overrides_client_level_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_millis(250),
        status: 200,
        headers: vec![],
        body: b"late".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(5));

    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client
        .request(Method::GET, "/request-write-timeout")
        .response_header_timeout(Duration::from_millis(80))
        .expect("positive write timeout should be accepted")
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::ResponseHeaderTimeout);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}

#[tokio_test]
async fn test_timeout_classification_is_retryable_in_deterministic_path() {
    let server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_millis(250),
        status: 200,
        headers: vec![],
        body: b"late".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_millis(80));

    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client.request(Method::GET, "/deterministic-timeout").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::RequestTimeout);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}

#[tokio_test]
async fn test_truncated_body_with_request_timeout_context_is_transport_error() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 200,
        headers: vec![],
        total_length: 16,
        prefix: b"abc".to_vec(),
        delay: Duration::from_millis(250),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(5));

    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client.request(Method::GET, "/request-timeout-read-phase").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let error = response.bytes().await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Transport);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}

#[tokio_test]
async fn test_reqwest_timeout_during_body_chunk_is_classified_as_read_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        chunks: vec![
            ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"first".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_millis(250),
                bytes: b"second".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_millis(80));

    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client.request(Method::GET, "/request-timeout-body-chunk").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let error = response.bytes().await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::ReadTimeout);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/request-timeout-body-chunk");
}

#[tokio_test]
async fn test_request_level_read_timeout_overrides_client_level_for_buffered_execute() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 200,
        headers: vec![],
        total_length: 16,
        prefix: b"abc".to_vec(),
        delay: Duration::from_millis(250),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(5));

    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client
        .request(Method::GET, "/request-read-timeout-override-buffered")
        .read_timeout(Duration::from_millis(80))
        .expect("positive read timeout should be accepted")
        .build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let error = response.bytes().await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::ReadTimeout);
}

#[tokio_test]
async fn test_request_level_read_timeout_overrides_client_level_for_stream_body() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        chunks: vec![
            ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"first".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_millis(250),
                bytes: b"second".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(5));

    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client
        .request(Method::GET, "/request-read-timeout-override-stream")
        .read_timeout(Duration::from_millis(80))
        .expect("positive read timeout should be accepted")
        .build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should start");

    let mut stream = response.stream().expect("stream body should be available");
    let first = stream
        .next()
        .await
        .expect("first chunk should exist")
        .expect("first chunk should be ok");
    assert_eq!(first, b"first".as_slice());

    let timeout_error = stream
        .next()
        .await
        .expect("second stream item should exist")
        .expect_err("second stream item should be read timeout");
    assert_eq!(timeout_error.kind, HttpErrorKind::ReadTimeout);
}

#[tokio_test]
async fn test_buffered_bytes_read_timeout_is_applied_per_chunk_wait() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        chunks: vec![
            ResponseChunk {
                delay: Duration::from_millis(1_200),
                bytes: b"first".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_millis(1_200),
                bytes: b"second".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let request = client.request(Method::GET, "/bytes-per-chunk-timeout").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should start");
    let body = response.bytes().await.expect("buffered body should be readable");
    assert_eq!(body, b"firstsecond".as_slice());

    let _ = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
}
