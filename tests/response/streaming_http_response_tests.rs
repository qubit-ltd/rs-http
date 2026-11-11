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
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use qubit_http::HttpResponse;
use qubit_redact::RedactionPolicy;
use qubit_redact::Sensitivity;
use tokio::test as tokio_test;
use tokio::time::timeout;
use url::Url;

use crate::common::ResponsePlan;
use crate::common::spawn_one_shot_server;

const CONSUMED_CONTEXT_SECRET: &str = "consumed-context-secret";

fn assert_consumed_error_context(
    error: &qubit_http::HttpError,
    expected_method: &Method,
    expected_url: &Url,
    expected_status: StatusCode,
) {
    assert_eq!(error.kind, HttpErrorKind::ResponseBodyAlreadyConsumed);
    assert_eq!(error.method.as_ref(), Some(expected_method));
    assert_eq!(error.url.as_ref(), Some(expected_url));
    assert_eq!(error.status, Some(expected_status));
    assert_eq!(
        error.url.as_ref().unwrap().query(),
        Some("tenant_marker=consumed-context-secret")
    );
    assert!(!format!("{error:?}").contains(CONSUMED_CONTEXT_SECRET));
    assert!(!error.to_string().contains(CONSUMED_CONTEXT_SECRET));
}

fn consumed_context_policy() -> RedactionPolicy {
    RedactionPolicy::default()
        .to_builder()
        .http(|http| {
            let _ = http.query().raise("tenant_marker", Sensitivity::High);
        })
        .expect("redaction policy should be valid")
        .build()
        .expect("redaction policy should be valid")
}

async fn response_with_gated_body() -> (
    HttpResponse,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let server =
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
            }
            socket.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n",
        ).await.unwrap();
            socket.flush().await.unwrap();
            let _ = release_rx.await;
            let _ = socket.write_all(b"{}").await;
        });
    let mut options = HttpClientOptions::default();
    options.log_redaction_policy = consumed_context_policy();
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client
        .request(
            Method::GET,
            &format!("http://{address}/gated?tenant_marker={CONSUMED_CONTEXT_SECRET}"),
        )
        .build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .unwrap()
        .unwrap();
    (response, release_tx, server)
}

#[tokio_test]
async fn test_dropped_pending_aggregate_read_consumes_backend() {
    for reader in 0..3 {
        let (mut response, release, server) = response_with_gated_body().await;
        let expected_method = response.meta().method().clone();
        let expected_url = response.request_url().clone();
        let expected_status = response.status();
        let mut future: std::pin::Pin<Box<dyn std::future::Future<Output = qubit_http::HttpResult<()>> + '_>> =
            match reader {
                0 => Box::pin(async { response.bytes().await.map(|_| ()) }),
                1 => Box::pin(async { response.text().await.map(|_| ()) }),
                _ => Box::pin(async { response.json::<serde_json::Value>().await.map(|_| ()) }),
            };
        assert!(matches!(futures_util::poll!(future.as_mut()), std::task::Poll::Pending));
        drop(future);
        let bytes_error = response.bytes().await.unwrap_err();
        assert_consumed_error_context(&bytes_error, &expected_method, &expected_url, expected_status);
        let stream_error = match response.stream() {
            Err(error) => error,
            Ok(_) => panic!("dropped aggregation must not restore the backend"),
        };
        assert_consumed_error_context(&stream_error, &expected_method, &expected_url, expected_status);
        drop(response);
        let _ = release.send(());
        timeout(Duration::from_secs(3), server).await.unwrap().unwrap();
    }
}

#[tokio_test]
async fn test_unpolled_aggregate_read_does_not_consume_backend() {
    let (mut response, release, server) = response_with_gated_body().await;
    let future = response.bytes();
    drop(future);
    release.send(()).unwrap();
    let body = timeout(Duration::from_secs(3), response.bytes())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(body.as_ref(), b"{}");
    timeout(Duration::from_secs(3), server).await.unwrap().unwrap();
}

#[test]
fn test_http_stream_response_is_success_and_new() {
    let response = HttpResponse::new(
        StatusCode::CREATED,
        HeaderMap::new(),
        Bytes::from_static(b"ok"),
        Url::parse("https://example.com/stream").unwrap(),
        Method::GET,
    );

    assert!(response.is_success());
    let debug = format!("{:?}", response);
    assert!(debug.contains("HttpResponse"));
    assert!(debug.contains("status"));
    assert!(debug.contains("headers"));
    assert!(debug.contains("url"));
}

#[tokio_test]
async fn test_http_stream_response_into_stream_consumes_body() {
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"part-1part-2"),
        Url::parse("https://example.com/stream").unwrap(),
        Method::GET,
    );

    let mut stream = response.stream().expect("stream body should be available");
    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        chunks.push(item.expect("stream item should decode"));
    }

    assert_eq!(chunks, vec![Bytes::from_static(b"part-1part-2")]);
}

#[tokio_test]
async fn test_http_stream_response_backend_taken_then_repeated_reads_fail() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        body: b"stream-from-backend".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.logging.log_response_body = false;
    options.log_redaction_policy = consumed_context_policy();
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let request = client
        .request(
            Method::GET,
            &format!("/stream-take-backend?tenant_marker={CONSUMED_CONTEXT_SECRET}"),
        )
        .build();
    let mut response = client.execute(request).await.expect("request should succeed");
    let expected_method = response.meta().method().clone();
    let expected_url = response.request_url().clone();
    let expected_status = response.status();

    let mut first_stream = response.stream().expect("first stream should take backend response");
    let mut collected = Vec::new();
    while let Some(item) = first_stream.next().await {
        collected.extend_from_slice(&item.expect("backend stream item should succeed"));
    }
    assert_eq!(collected, b"stream-from-backend");

    let second_error = match response.stream() {
        Err(error) => error,
        Ok(_) => panic!("second stream must report consumed body"),
    };
    assert_consumed_error_context(&second_error, &expected_method, &expected_url, expected_status);
    let bytes_error = response
        .bytes()
        .await
        .expect_err("bytes after stream must report consumed body");
    assert_consumed_error_context(&bytes_error, &expected_method, &expected_url, expected_status);

    let captured = server.finish().await;
    assert_eq!(
        captured.target,
        format!("/stream-take-backend?tenant_marker={CONSUMED_CONTEXT_SECRET}")
    );
}

#[tokio_test]
async fn test_http_response_bytes_remembers_read_failure() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 200,
        headers: vec![],
        total_length: 8,
        prefix: b"abc".to_vec(),
        delay: Duration::from_millis(0),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.logging.log_response_body = false;
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let request = client.request(Method::GET, "/bytes-read-failure").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should start");
    let first_error = response
        .bytes()
        .await
        .expect_err("truncated body should fail first read");
    assert_eq!(first_error.kind, HttpErrorKind::Transport);
    assert_eq!(first_error.status, Some(StatusCode::OK));

    let second_error = response
        .bytes()
        .await
        .expect_err("second read should preserve the prior body read failure");
    assert_eq!(second_error.kind, HttpErrorKind::Transport);
    assert_eq!(second_error.status, Some(StatusCode::OK));
    assert!(second_error.message.contains("previous response body read failed"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/bytes-read-failure");
}

#[tokio_test]
async fn test_http_response_stream_remembers_read_failure() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 200,
        headers: vec![],
        total_length: 8,
        prefix: b"abc".to_vec(),
        delay: Duration::from_millis(0),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.logging.log_response_body = false;
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let request = client.request(Method::GET, "/stream-read-failure").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should start");
    let mut stream = response.stream().expect("stream body should be available");

    let first = stream
        .next()
        .await
        .expect("first stream item should exist")
        .expect("first stream item should be bytes");
    assert_eq!(first, Bytes::from_static(b"abc"));
    let stream_error = stream
        .next()
        .await
        .expect("second stream item should contain read error")
        .expect_err("truncated stream should fail");
    assert_eq!(stream_error.kind, HttpErrorKind::Transport);
    assert_eq!(stream_error.status, Some(StatusCode::OK));
    drop(stream);

    let second_error = response
        .bytes()
        .await
        .expect_err("bytes after stream failure should preserve the read failure");
    assert_eq!(second_error.kind, HttpErrorKind::Transport);
    assert_eq!(second_error.status, Some(StatusCode::OK));
    assert!(second_error.message.contains("previous response body read failed"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/stream-read-failure");
}

#[tokio_test]
async fn test_http_response_stream_reports_prior_bytes_read_failure() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 200,
        headers: vec![],
        total_length: 8,
        prefix: b"abc".to_vec(),
        delay: Duration::from_millis(0),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.logging.log_response_body = false;
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let request = client.request(Method::GET, "/stream-after-bytes-failure").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should start");
    let first_error = response
        .bytes()
        .await
        .expect_err("truncated body should fail first read");
    assert_eq!(first_error.kind, HttpErrorKind::Transport);

    let stream_error = match response.stream() {
        Ok(_) => panic!("stream should preserve the prior body read failure"),
        Err(error) => error,
    };
    assert_eq!(stream_error.kind, HttpErrorKind::Transport);
    assert_eq!(stream_error.status, Some(StatusCode::OK));
    assert!(stream_error.message.contains("previous response body read failed"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/stream-after-bytes-failure");
}
