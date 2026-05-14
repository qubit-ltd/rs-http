/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::sync::{
    atomic::{
        AtomicUsize,
        Ordering,
    },
    Arc,
};
use std::time::Duration;

use bytes::Bytes;
use futures_util::stream;
use http::Method;
use qubit_http::{
    HttpClientFactory,
    HttpClientOptions,
    HttpRequestBodyByteStream,
    HttpRetryMethodPolicy,
    RetryDelay,
};
use tokio::time::timeout;

use crate::common::{
    spawn_multi_shot_server,
    spawn_one_shot_server,
    ResponsePlan,
};

#[tokio::test]
async fn test_execute_with_form_body_and_query_headers_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let request = client
        .request(Method::POST, "/form")
        .query_param("kind", "form")
        .header("x-test", "1")
        .expect("header should be valid")
        .form_body([("name", "alice"), ("city", "shanghai")])
        .request_timeout(Duration::from_secs(1))
        .expect("positive request timeout should be accepted")
        .build();
    timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("form request should succeed");

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/form?kind=form");
    assert_eq!(
        captured.headers.get("content-type"),
        Some(&"application/x-www-form-urlencoded".to_string())
    );
    let body = String::from_utf8(captured.body).expect("body should be utf-8");
    assert!(body.contains("name=alice"));
    assert!(body.contains("city=shanghai"));
}

#[tokio::test]
async fn test_execute_with_multipart_body_and_query_headers_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let payload = Bytes::from_static(
        b"--abc\r\nContent-Disposition: form-data; name=\"field\"\r\n\r\nvalue\r\n--abc--\r\n",
    );
    let request = client
        .request(Method::POST, "/multipart")
        .query_param("kind", "multipart")
        .header("x-test", "1")
        .expect("header should be valid")
        .multipart_body(payload.clone(), "abc")
        .expect("multipart body should be built")
        .request_timeout(Duration::from_secs(1))
        .expect("positive request timeout should be accepted")
        .build();
    timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("multipart request should succeed");

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/multipart?kind=multipart");
    assert_eq!(
        captured.headers.get("content-type"),
        Some(&"multipart/form-data; boundary=abc".to_string())
    );
    assert_eq!(captured.body, payload.to_vec());
}

#[tokio::test]
async fn test_execute_with_ndjson_body_and_query_headers_timeout() {
    #[derive(serde::Serialize)]
    struct Record {
        id: i32,
        name: &'static str,
    }

    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let request = client
        .request(Method::POST, "/ndjson")
        .query_param("kind", "ndjson")
        .header("x-test", "1")
        .expect("header should be valid")
        .ndjson_body(&[
            Record {
                id: 1,
                name: "alpha",
            },
            Record {
                id: 2,
                name: "beta",
            },
        ])
        .expect("ndjson should be encoded")
        .request_timeout(Duration::from_secs(1))
        .expect("positive request timeout should be accepted")
        .build();
    timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("ndjson request should succeed");

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/ndjson?kind=ndjson");
    assert_eq!(
        captured.headers.get("content-type"),
        Some(&"application/x-ndjson".to_string())
    );
    let body = String::from_utf8(captured.body).expect("body should be utf-8");
    assert_eq!(
        body,
        "{\"id\":1,\"name\":\"alpha\"}\n{\"id\":2,\"name\":\"beta\"}\n"
    );
}

#[tokio::test]
async fn test_execute_with_stream_body_uses_chunked_transfer_encoding() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let request = client
        .request(Method::POST, "/stream-upload")
        .query_param("kind", "stream")
        .stream_body([
            Bytes::from_static(b"first-"),
            Bytes::from_static(b"second-"),
            Bytes::from_static(b"third"),
        ])
        .request_timeout(Duration::from_secs(1))
        .expect("positive request timeout should be accepted")
        .build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("stream body request should succeed");
    assert_eq!(response.status().as_u16(), 200);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/stream-upload?kind=stream");
    assert_eq!(
        captured.headers.get("transfer-encoding"),
        Some(&"chunked".to_string())
    );
}

#[tokio::test]
async fn test_execute_with_stream_body_uses_chunked_transfer_encoding_without_eager_read() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let request = client
        .request(Method::POST, "/stream-upload-streaming")
        .query_param("kind", "stream-body")
        .stream_body([Bytes::from_static(b"a"), Bytes::from_static(b"b")])
        .request_timeout(Duration::from_secs(1))
        .expect("positive request timeout should be accepted")
        .build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("stream body request should succeed");
    assert_eq!(response.status().as_u16(), 200);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/stream-upload-streaming?kind=stream-body");
    assert_eq!(
        captured.headers.get("transfer-encoding"),
        Some(&"chunked".to_string())
    );
}

#[tokio::test]
async fn test_execute_with_streaming_body_factory_supports_retry_rebuild() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 503,
            headers: vec![],
            body: b"retry".to_vec(),
        },
        ResponsePlan::Immediate {
            status: 200,
            headers: vec![],
            body: b"ok".to_vec(),
        },
    ])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.delay_strategy = RetryDelay::None;
    options.retry.method_policy = HttpRetryMethodPolicy::AllMethods;
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let stream_factory_calls = Arc::new(AtomicUsize::new(0));
    let stream_factory_calls_for_builder = Arc::clone(&stream_factory_calls);
    let request = client
        .request(Method::POST, "/streaming-body-factory")
        .streaming_body(move || {
            let stream_factory_calls_for_future = Arc::clone(&stream_factory_calls_for_builder);
            Box::pin(async move {
                stream_factory_calls_for_future.fetch_add(1, Ordering::Relaxed);
                Box::pin(stream::iter(vec![
                    Ok(Bytes::from_static(b"part-1-")),
                    Ok(Bytes::from_static(b"part-2")),
                ])) as HttpRequestBodyByteStream
            })
        })
        .build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should succeed after retry");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(stream_factory_calls.load(Ordering::Relaxed), 2);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
    assert_eq!(
        captured[0].headers.get("transfer-encoding"),
        Some(&"chunked".to_string())
    );
    assert_eq!(
        captured[1].headers.get("transfer-encoding"),
        Some(&"chunked".to_string())
    );
}

#[tokio::test]
async fn test_streaming_body_factory_preparation_respects_write_timeout() {
    let server = spawn_multi_shot_server(vec![]).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_millis(50);
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let request = client
        .request(Method::POST, "/streaming-body-timeout")
        .streaming_body(|| {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Box::pin(stream::empty::<Result<Bytes, std::io::Error>>())
                    as HttpRequestBodyByteStream
            })
        })
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("streaming body preparation should hit write timeout");

    assert_eq!(error.kind, qubit_http::HttpErrorKind::WriteTimeout);
    assert!(error.message.contains("streaming request body"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert!(captured.is_empty());
}
