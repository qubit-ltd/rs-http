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
use qubit_http::HttpClientFactory;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use qubit_http::HttpResponse;
use tokio::time::timeout;
use url::Url;

use crate::common::ResponsePlan;
use crate::common::spawn_one_shot_server;

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

#[tokio::test]
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

#[tokio::test]
async fn test_http_stream_response_backend_taken_then_stream_and_bytes_are_empty() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        body: b"stream-from-backend".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.logging.log_response_body = false;
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let request = client.request(Method::GET, "/stream-take-backend").build();
    let mut response = client.execute(request).await.expect("request should succeed");

    let mut first_stream = response.stream().expect("first stream should take backend response");
    let first_chunk = first_stream
        .next()
        .await
        .expect("first stream should yield one item")
        .expect("first stream chunk should be valid");
    assert_eq!(first_chunk, Bytes::from_static(b"stream-from-backend"));
    assert!(
        first_stream.next().await.is_none(),
        "first stream should end after one chunk"
    );

    let mut second_stream = response
        .stream()
        .expect("second stream should still return an empty stream");
    assert!(
        second_stream.next().await.is_none(),
        "second stream should be empty after backend is already taken"
    );

    let bytes = response
        .bytes()
        .await
        .expect("bytes after backend taken should resolve to empty");
    assert!(bytes.is_empty());

    let captured = server.finish().await;
    assert_eq!(captured.target, "/stream-take-backend");
}

#[tokio::test]
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
    let client = HttpClientFactory::new()
        .create(options)
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

#[tokio::test]
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
    let client = HttpClientFactory::new()
        .create(options)
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

#[tokio::test]
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
    let client = HttpClientFactory::new()
        .create(options)
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
