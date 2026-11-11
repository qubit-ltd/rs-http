// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use bytes::Bytes;
use futures_util::stream;
use http::Method;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpRequestBody;
use qubit_http::HttpRequestBodyByteStream;
use qubit_http::HttpRequestStreamingBody;

#[test]
fn test_http_request_streaming_body_sets_request_body_to_empty_placeholder() {
    let streaming_body = HttpRequestStreamingBody::new(|| {
        Box::pin(
            async move { Box::pin(stream::iter(vec![Ok(Bytes::from_static(b"chunk"))])) as HttpRequestBodyByteStream },
        )
    });
    assert!(format!("{streaming_body:?}").contains("HttpRequestStreamingBody"));

    let client = HttpClientBuilder::new()
        .build()
        .expect("default client should be created");
    let request = client
        .request(Method::POST, "https://example.com/upload")
        .streaming_body(move || {
            Box::pin(async move {
                Box::pin(stream::iter(vec![Ok(Bytes::from_static(b"chunk"))])) as HttpRequestBodyByteStream
            })
        })
        .build();

    assert_eq!(request.body(), &HttpRequestBody::Empty);
}

#[test]
fn test_try_new_is_lazy_cloneable_and_debuggable() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_factory = Arc::clone(&calls);
    let body = HttpRequestStreamingBody::try_new(move || {
        calls_for_factory.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Ok(Box::pin(stream::empty()) as HttpRequestBodyByteStream) })
    });
    let clone = body.clone();
    assert!(format!("{clone:?}").contains("HttpRequestStreamingBody"));
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn test_try_streaming_body_replaces_body_and_content_type_like_existing_helper() {
    let client = HttpClientBuilder::new().build().unwrap();
    let generated = client
        .request(Method::POST, "https://example.com/upload")
        .json_body(&serde_json::json!({"old": true}))
        .unwrap()
        .try_streaming_body(|| Box::pin(async { Ok(Box::pin(stream::empty()) as HttpRequestBodyByteStream) }))
        .build();
    assert_eq!(generated.body(), &HttpRequestBody::Empty);
    assert!(!generated.headers().contains_key(http::header::CONTENT_TYPE));

    let explicit = client
        .request(Method::POST, "https://example.com/upload")
        .header("content-type", "application/custom")
        .unwrap()
        .try_streaming_body(|| Box::pin(async { Ok(Box::pin(stream::empty()) as HttpRequestBodyByteStream) }))
        .bytes_body(Bytes::from_static(b"replacement"))
        .build();
    assert_eq!(
        explicit.body(),
        &HttpRequestBody::Bytes(Bytes::from_static(b"replacement"))
    );
    assert_eq!(explicit.headers()[http::header::CONTENT_TYPE], "application/custom");
}
