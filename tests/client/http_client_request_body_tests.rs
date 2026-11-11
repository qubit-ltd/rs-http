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
use std::time::Duration;

use bytes::Bytes;
use futures_util::stream;
use http::Method;
use qubit_http::HttpCancellationToken;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpError;
use qubit_http::HttpErrorKind;
use qubit_http::HttpRequestBodyByteStream;
use qubit_http::HttpRetryMethodPolicy;
use qubit_retry::BackoffPolicy;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::test as tokio_test;
use tokio::time::sleep;
use tokio::time::timeout;

use crate::common::CapturedRequest;
use crate::common::ResponsePlan;
use crate::common::read_request;
use crate::common::spawn_multi_shot_server;
use crate::common::spawn_one_shot_server;

#[tokio_test]
async fn test_try_streaming_body_preparation_error_sends_nothing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/upload", listener.local_addr().unwrap());
    let client = HttpClientBuilder::new().build().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_factory = Arc::clone(&calls);
    let request = client
        .request(Method::POST, &url)
        .try_streaming_body(move || {
            calls_for_factory.fetch_add(1, Ordering::Relaxed);
            Box::pin(async {
                Err(HttpError::decode(
                    "failed to prepare https://alice:pwsecret@example.test/?access_token=tokensecret",
                )
                .with_source(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "cannot open upload",
                )))
            })
        })
        .disable_retry()
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind(), HttpErrorKind::Decode);
    assert!(std::error::Error::source(&error).is_some());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(!format!("{error}\n{error:?}").contains("pwsecret"));
    assert!(!format!("{error}\n{error:?}").contains("tokensecret"));
    assert!(timeout(Duration::from_millis(100), listener.accept()).await.is_err());
}

#[tokio_test]
async fn test_try_streaming_body_pending_preparation_times_out_without_connecting() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/upload", listener.local_addr().unwrap());
    let mut options = HttpClientOptions::default();
    options.timeouts.response_header_timeout = Duration::from_millis(50);
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client
        .request(Method::POST, &url)
        .try_streaming_body(|| {
            Box::pin(async { std::future::pending::<qubit_http::HttpResult<HttpRequestBodyByteStream>>().await })
        })
        .disable_retry()
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind(), HttpErrorKind::ResponseHeaderTimeout);
    assert!(timeout(Duration::from_millis(100), listener.accept()).await.is_err());
}

#[tokio_test]
async fn test_try_streaming_body_upload_and_factory_runs_once() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_factory = Arc::clone(&calls);
    let client = HttpClientBuilder::new().build().unwrap();
    let request = client
        .request(Method::POST, server.base_url().as_str())
        .try_streaming_body(move || {
            calls_for_factory.fetch_add(1, Ordering::Relaxed);
            Box::pin(async {
                Ok(Box::pin(stream::iter([Ok(Bytes::from_static(b"upload"))])) as HttpRequestBodyByteStream)
            })
        })
        .disable_retry()
        .build();
    timeout(Duration::from_secs(3), client.execute(request))
        .await
        .unwrap()
        .unwrap();
    let captured = timeout(Duration::from_secs(3), server.finish()).await.unwrap();
    assert_eq!(captured.body, b"upload");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[tokio_test]
async fn test_try_streaming_body_pending_preparation_is_cancelled_without_connecting() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/upload", listener.local_addr().unwrap());
    let token = HttpCancellationToken::new();
    let client = HttpClientBuilder::new().build().unwrap();
    let request = client
        .request(Method::POST, &url)
        .try_streaming_body(|| {
            Box::pin(async { std::future::pending::<qubit_http::HttpResult<HttpRequestBodyByteStream>>().await })
        })
        .cancellation_token(token.clone())
        .disable_retry()
        .build();
    let execute = tokio::spawn(async move { client.execute(request).await });
    sleep(Duration::from_millis(20)).await;
    token.cancel();
    let error = timeout(Duration::from_secs(3), execute)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind(), HttpErrorKind::Cancelled);
    assert!(timeout(Duration::from_millis(100), listener.accept()).await.is_err());
}

#[tokio_test]
async fn test_try_streaming_body_transport_preparation_retries_factory() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    options.retry.method_policy = HttpRetryMethodPolicy::AllMethods;
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_factory = Arc::clone(&calls);
    let request = client
        .request(Method::POST, server.base_url().as_str())
        .try_streaming_body(move || {
            let attempt = calls_for_factory.fetch_add(1, Ordering::Relaxed);
            Box::pin(async move {
                if attempt == 0 {
                    Err(HttpError::transport("temporary preparation failure"))
                } else {
                    Ok(Box::pin(stream::iter([Ok(Bytes::from_static(b"upload"))])) as HttpRequestBodyByteStream)
                }
            })
        })
        .build();
    timeout(Duration::from_secs(3), client.execute(request))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    let captured = timeout(Duration::from_secs(3), server.finish()).await.unwrap();
    assert_eq!(captured.body, b"upload");
}

#[tokio_test]
async fn test_upload_redirect_307_bytes_body_follows_and_preserves_post() {
    run_upload_redirect(307, UploadBodyKind::Bytes).await;
}

#[tokio_test]
async fn test_upload_redirect_308_bytes_body_follows_and_preserves_post() {
    run_upload_redirect(308, UploadBodyKind::Bytes).await;
}

#[tokio_test]
async fn test_upload_redirect_307_stream_body_returns_status_without_following() {
    run_upload_redirect(307, UploadBodyKind::Stream).await;
}

#[tokio_test]
async fn test_upload_redirect_308_stream_body_returns_status_without_following() {
    run_upload_redirect(308, UploadBodyKind::Stream).await;
}

#[tokio_test]
async fn test_upload_redirect_307_streaming_body_returns_status_without_following() {
    run_upload_redirect(307, UploadBodyKind::Factory).await;
}

#[tokio_test]
async fn test_upload_redirect_308_streaming_body_returns_status_without_following() {
    run_upload_redirect(308, UploadBodyKind::Factory).await;
}

#[derive(Clone, Copy)]
enum UploadBodyKind {
    Bytes,
    Stream,
    Factory,
}

async fn run_upload_redirect(status: u16, body_kind: UploadBodyKind) {
    let expected_count = usize::from(matches!(body_kind, UploadBodyKind::Bytes)) + 1;
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener should bind");
    let addr = listener.local_addr().expect("listener address should exist");
    let base_url = format!("http://{addr}/");
    let server: JoinHandle<Vec<CapturedRequest>> = tokio::spawn(async move {
        let mut requests = Vec::with_capacity(expected_count);
        for index in 0..expected_count {
            let (mut socket, _) = timeout(Duration::from_secs(3), listener.accept())
                .await
                .expect("server accept timed out")
                .expect("server accept failed");
            let request = timeout(Duration::from_secs(3), read_request(&mut socket))
                .await
                .expect("request read timed out")
                .expect("request read failed");
            let response = if index == 0 {
                format!(
                    "HTTP/1.1 {status} Redirect\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
            };
            socket
                .write_all(response.as_bytes())
                .await
                .expect("response write failed");
            requests.push(request);
        }
        requests
    });
    let mut options = HttpClientOptions::default();
    options.base_url = Some(url::Url::parse(&base_url).expect("base URL should parse"));
    options.max_redirects = Some(5);
    options.retry.enabled = false;
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let mut builder = client.request(Method::POST, "/upload");
    builder = match body_kind {
        UploadBodyKind::Bytes => builder.bytes_body(Bytes::from_static(b"payload")),
        UploadBodyKind::Stream => builder.stream_body([Bytes::from_static(b"payload")]),
        UploadBodyKind::Factory => {
            let calls = Arc::clone(&factory_calls);
            builder.streaming_body(move || {
                calls.fetch_add(1, Ordering::Relaxed);
                Box::pin(async {
                    Box::pin(stream::iter(vec![Ok(Bytes::from_static(b"payload"))])) as HttpRequestBodyByteStream
                })
            })
        }
    };
    let request = builder.disable_retry().build();
    let result = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("upload request timed out");
    match body_kind {
        UploadBodyKind::Bytes => assert_eq!(result.expect("buffered redirect should succeed").status().as_u16(), 200),
        UploadBodyKind::Stream | UploadBodyKind::Factory => {
            let error = result.expect_err("streaming redirect should not replay its body");
            assert_eq!(error.kind(), HttpErrorKind::Status);
            assert_eq!(error.status().expect("status should be present").as_u16(), status);
        }
    }
    let captured = timeout(Duration::from_secs(3), server)
        .await
        .expect("server task timed out")
        .expect("server task failed");
    assert_eq!(captured.len(), expected_count);
    assert_eq!(captured[0].method, "POST");
    assert_eq!(captured[0].target, "/upload");
    assert_eq!(captured[0].body, b"payload");
    if expected_count == 2 {
        assert_eq!(captured[1].method, "POST");
        assert_eq!(captured[1].target, "/final");
        assert_eq!(captured[1].body, b"payload");
    }
    if matches!(body_kind, UploadBodyKind::Factory) {
        assert_eq!(factory_calls.load(Ordering::Relaxed), 1);
    }
}

#[tokio_test]
async fn test_execute_with_form_body_and_query_headers_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
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

#[tokio_test]
async fn test_execute_with_multipart_body_and_query_headers_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let payload =
        Bytes::from_static(b"--abc\r\nContent-Disposition: form-data; name=\"field\"\r\n\r\nvalue\r\n--abc--\r\n");
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

#[tokio_test]
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
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let request = client
        .request(Method::POST, "/ndjson")
        .query_param("kind", "ndjson")
        .header("x-test", "1")
        .expect("header should be valid")
        .ndjson_body(&[Record { id: 1, name: "alpha" }, Record { id: 2, name: "beta" }])
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
    assert_eq!(body, "{\"id\":1,\"name\":\"alpha\"}\n{\"id\":2,\"name\":\"beta\"}\n");
}

#[tokio_test]
async fn test_execute_with_stream_body_uses_chunked_transfer_encoding() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let large_chunk = vec![b'x'; 2048];
    let request = client
        .request(Method::POST, "/stream-upload")
        .query_param("kind", "stream")
        .stream_body([Bytes::from(large_chunk.clone()), Bytes::from_static(b"tail")])
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
    assert_eq!(captured.headers.get("transfer-encoding"), Some(&"chunked".to_string()));
    let mut expected = large_chunk;
    expected.extend_from_slice(b"tail");
    assert_eq!(captured.body, expected);
}

#[tokio_test]
async fn test_execute_with_stream_body_preserves_ordered_chunks() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
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
    assert_eq!(captured.headers.get("transfer-encoding"), Some(&"chunked".to_string()));
    assert_eq!(captured.body, b"ab");
}

#[tokio_test]
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
    options.retry.backoff = BackoffPolicy::immediate();
    options.retry.method_policy = HttpRetryMethodPolicy::AllMethods;
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let stream_factory_calls = Arc::new(AtomicUsize::new(0));
    let stream_factory_entries = Arc::new(AtomicUsize::new(0));
    let stream_factory_calls_for_builder = Arc::clone(&stream_factory_calls);
    let stream_factory_entries_for_builder = Arc::clone(&stream_factory_entries);
    let request = client
        .request(Method::POST, "/streaming-body-factory")
        .streaming_body(move || {
            stream_factory_entries_for_builder.fetch_add(1, Ordering::Relaxed);
            let stream_factory_calls_for_future = Arc::clone(&stream_factory_calls_for_builder);
            Box::pin(async move {
                stream_factory_calls_for_future.fetch_add(1, Ordering::Relaxed);
                Box::pin(stream::unfold(0, |index| async move {
                    let chunk = match index {
                        0 => Some(Bytes::from_static(b"part-1-")),
                        1 => Some(Bytes::from_static(b"part-2")),
                        2 => Some(Bytes::from_static(b"part-3")),
                        _ => None,
                    }?;
                    sleep(Duration::from_millis(5)).await;
                    Some((Ok(chunk), index + 1))
                })) as HttpRequestBodyByteStream
            })
        })
        .build();
    assert_eq!(stream_factory_entries.load(Ordering::Relaxed), 0);
    assert_eq!(stream_factory_calls.load(Ordering::Relaxed), 0);

    let response = timeout(Duration::from_secs(15), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should succeed after retry");
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(stream_factory_entries.load(Ordering::Relaxed), 2);
    assert_eq!(stream_factory_calls.load(Ordering::Relaxed), 2);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
    for request in &captured {
        assert_eq!(request.body, b"part-1-part-2part-3");
    }
    assert_eq!(
        captured[0].headers.get("transfer-encoding"),
        Some(&"chunked".to_string())
    );
    assert_eq!(
        captured[1].headers.get("transfer-encoding"),
        Some(&"chunked".to_string())
    );
}

#[tokio_test]
async fn test_streaming_body_factory_preparation_respects_response_header_timeout() {
    let server = spawn_multi_shot_server(vec![]).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.response_header_timeout = Duration::from_millis(50);
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let request = client
        .request(Method::POST, "/streaming-body-timeout")
        .streaming_body(|| {
            Box::pin(async move {
                sleep(Duration::from_secs(5)).await;
                Box::pin(stream::empty::<Result<Bytes, std::io::Error>>()) as HttpRequestBodyByteStream
            })
        })
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("streaming body preparation should hit write timeout");

    assert_eq!(error.kind, HttpErrorKind::ResponseHeaderTimeout);
    assert!(error.message.contains("streaming request body"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert!(captured.is_empty());
}
