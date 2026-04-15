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
use qubit_http::StreamingHttpResponse;
use url::Url;

#[test]
fn test_http_stream_response_is_success_and_new() {
    let response = StreamingHttpResponse::new_stream(
        StatusCode::CREATED,
        HeaderMap::new(),
        Url::parse("https://example.com/stream").unwrap(),
        Box::pin(futures_util::stream::once(async {
            Ok(Bytes::from_static(b"ok"))
        })),
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
    let response = StreamingHttpResponse::new_stream(
        StatusCode::OK,
        HeaderMap::new(),
        Url::parse("https://example.com/stream").unwrap(),
        Box::pin(futures_util::stream::iter(vec![
            Ok(Bytes::from_static(b"part-1")),
            Ok(Bytes::from_static(b"part-2")),
        ])),
        Method::GET,
    );

    let mut stream = response.into_stream();
    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        chunks.push(item.expect("stream item should decode"));
    }

    assert_eq!(chunks, vec![b"part-1".to_vec(), b"part-2".to_vec()]);
}
