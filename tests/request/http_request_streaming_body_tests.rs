// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use bytes::Bytes;
use futures_util::stream;
use http::Method;
use qubit_http::HttpClientFactory;
use qubit_http::HttpRequestBody;
use qubit_http::HttpRequestBodyByteStream;
use qubit_http::HttpRequestStreamingBody;

#[test]
fn test_http_request_streaming_body_sets_request_body_to_empty_placeholder() {
    let streaming_body = HttpRequestStreamingBody::new(|| {
        Box::pin(async move {
            Box::pin(stream::iter(vec![Ok(Bytes::from_static(b"chunk"))]))
                as HttpRequestBodyByteStream
        })
    });
    assert!(format!("{streaming_body:?}").contains("HttpRequestStreamingBody"));

    let client = HttpClientFactory::new()
        .create_default()
        .expect("default client should be created");
    let request = client
        .request(Method::POST, "https://example.com/upload")
        .streaming_body(move || {
            Box::pin(async move {
                Box::pin(stream::iter(vec![Ok(Bytes::from_static(b"chunk"))]))
                    as HttpRequestBodyByteStream
            })
        })
        .build();

    assert_eq!(request.body(), &HttpRequestBody::Empty);
}
