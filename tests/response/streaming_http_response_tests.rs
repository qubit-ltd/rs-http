/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use bytes::Bytes;
use futures_util::StreamExt;
use http::{HeaderMap, Method, StatusCode};
use qubit_http::HttpResponse;
use url::Url;

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

    let mut stream = response
        .stream_body()
        .expect("stream body should be available");
    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        chunks.push(item.expect("stream item should decode"));
    }

    assert_eq!(chunks, vec![Bytes::from_static(b"part-1part-2")]);
}
