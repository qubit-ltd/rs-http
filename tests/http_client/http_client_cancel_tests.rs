/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::Duration;

use futures_util::StreamExt;
use http::Method;
use qubit_http::{
    CancellationToken, HttpClientFactory, HttpClientOptions, HttpError, HttpErrorKind, RetryHint,
};
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

    let mut stream = response
        .stream()
        .expect("stream body should be available");
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
