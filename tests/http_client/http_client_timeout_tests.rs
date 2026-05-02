/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::time::Duration;

use futures_util::StreamExt;
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions, HttpErrorKind, RetryHint};
use tokio::time::timeout;

use crate::common::{spawn_one_shot_server, ResponseChunk, ResponsePlan};

#[tokio::test]
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
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_millis(80));

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client.request(Method::GET, "/request-timeout").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::RequestTimeout);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}

#[tokio::test]
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
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(5));

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client
        .request(Method::GET, "/request-timeout-override")
        .request_timeout(Duration::from_millis(80))
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::RequestTimeout);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}

#[tokio::test]
async fn test_client_level_write_timeout_triggers_write_timeout_error() {
    let server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_millis(250),
        status: 200,
        headers: vec![],
        body: b"late".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_millis(80);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(5));

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client.request(Method::GET, "/client-write-timeout").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::WriteTimeout);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}

#[tokio::test]
async fn test_request_level_write_timeout_overrides_client_level_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_millis(250),
        status: 200,
        headers: vec![],
        body: b"late".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(5));

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client
        .request(Method::GET, "/request-write-timeout")
        .write_timeout(Duration::from_millis(80))
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::WriteTimeout);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}

#[tokio::test]
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
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_millis(80));

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client
        .request(Method::GET, "/deterministic-timeout")
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::RequestTimeout);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}

#[tokio::test]
async fn test_request_timeout_during_body_read_is_classified_as_read_timeout() {
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
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(5));

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client
        .request(Method::GET, "/request-timeout-read-phase")
        .build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let error = response.bytes().await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert_eq!(error.retry_hint(), RetryHint::NonRetryable);
}

#[tokio::test]
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
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(5));

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client
        .request(Method::GET, "/request-read-timeout-override-buffered")
        .read_timeout(Duration::from_millis(80))
        .build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let error = response.bytes().await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::ReadTimeout);
}

#[tokio::test]
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
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(5));

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client
        .request(Method::GET, "/request-read-timeout-override-stream")
        .read_timeout(Duration::from_millis(80))
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

#[tokio::test]
async fn test_buffered_bytes_read_timeout_is_applied_per_chunk_wait() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        chunks: vec![
            ResponseChunk {
                delay: Duration::from_millis(180),
                bytes: b"first".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_millis(180),
                bytes: b"second".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_millis(300);
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client
        .request(Method::GET, "/bytes-per-chunk-timeout")
        .build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should start");
    let body = response
        .bytes()
        .await
        .expect("buffered body should be readable");
    assert_eq!(body, b"firstsecond".as_slice());

    let _ = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
}
