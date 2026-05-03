/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use bytes::Bytes;
use futures_util::StreamExt;
use http::{
    HeaderMap,
    Method,
    StatusCode,
};
use qubit_http::{
    HttpClientFactory,
    HttpClientOptions,
    HttpResponse,
};
use url::Url;

use crate::common::{
    spawn_one_shot_server,
    ResponsePlan,
};

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
    let debug = format!("{:?}", &response);
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
    let mut response = client
        .execute(request)
        .await
        .expect("request should succeed");

    let mut first_stream = response
        .stream()
        .expect("first stream should take backend response");
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
