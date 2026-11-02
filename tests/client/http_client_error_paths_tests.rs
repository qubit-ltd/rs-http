// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use http::Method;
use http::StatusCode;
use httpdate::fmt_http_date;
use qubit_http::HttpCancellationToken;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use qubit_http::HttpRequest;
use qubit_http::HttpRequestInterceptor;
use qubit_http::HttpRetryTermination;
use qubit_retry::BackoffPolicy;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::test as tokio_test;
use tokio::time::timeout;

use crate::common::ResponsePlan;
use crate::common::spawn_one_shot_server;

#[tokio_test]
async fn test_non_http_scheme_does_not_retry() {
    let mut options = HttpClientOptions::default();
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.backoff = BackoffPolicy::immediate();
    let client = HttpClientBuilder::new().create(options).unwrap();
    let error = client
        .execute(client.request(Method::GET, "ftp://sensitive.invalid/path").build())
        .await
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::InvalidUrl);
    let diagnostics = error.retry_diagnostics().expect("retry diagnostics should be attached");
    assert_eq!(diagnostics.attempts(), 1);
    assert_eq!(diagnostics.termination(), HttpRetryTermination::Aborted);
    assert!(!error.message.contains("sensitive.invalid"));
}

#[tokio_test]
async fn test_interceptor_non_http_scheme_does_not_retry() {
    let mut options = HttpClientOptions::default();
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientBuilder::new().create(options).unwrap();
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let captured = calls.clone();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |request: &mut HttpRequest| {
        captured.fetch_add(1, Ordering::SeqCst);
        request.set_path("ftp://sensitive.invalid/path");
        Ok(())
    }));
    let error = client
        .execute(client.request(Method::GET, "http://example.invalid/").build())
        .await
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::InvalidUrl);
    let diagnostics = error.retry_diagnostics().expect("retry diagnostics should be attached");
    assert_eq!(diagnostics.attempts(), 1);
    assert_eq!(diagnostics.termination(), HttpRetryTermination::Aborted);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!error.message.contains("sensitive.invalid"));
}

#[tokio_test]
async fn test_execute_maps_retry_after_to_retryable_http_error() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 429,
        headers: vec![("Retry-After".to_string(), "2".to_string())],
        body: b"too many requests".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new()
        .create(options)
        .expect("client should be created");
    let request = client.request(Method::GET, "/limited").build();

    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("response status 429 should fail");

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::TOO_MANY_REQUESTS));
    assert_eq!(error.retry_after, Some(Duration::from_secs(2)));
    assert_eq!(
        error.response_body_preview.as_deref(),
        Some("<redacted: unsupported HTTP body>")
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/limited");
}

#[tokio_test]
async fn test_execute_parses_retry_after_http_date_for_service_unavailable() {
    let retry_after_value = fmt_http_date(std::time::SystemTime::now() + Duration::from_secs(3));
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 503,
        headers: vec![("Retry-After".to_string(), retry_after_value)],
        body: b"service unavailable".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new()
        .create(options)
        .expect("client should be created");
    let request = client.request(Method::GET, "/service-unavailable").build();

    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("response status 503 should fail");

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::SERVICE_UNAVAILABLE));
    let retry_after = error.retry_after.expect("Retry-After HTTP-date should be parsed");
    assert!(
        retry_after <= Duration::from_secs(3),
        "retry_after={retry_after:?} should not exceed remaining date delta"
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/service-unavailable");
}

#[tokio_test]
async fn test_execute_ignores_invalid_retry_after_for_service_unavailable() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 503,
        headers: vec![("Retry-After".to_string(), "not-a-valid-value".to_string())],
        body: b"service unavailable".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new()
        .create(options)
        .expect("client should be created");
    let request = client
        .request(Method::GET, "/service-unavailable-invalid-retry-after")
        .build();

    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("response status 503 should fail");

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(error.retry_after, None);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/service-unavailable-invalid-retry-after");
}

#[tokio_test]
async fn test_execute_ignores_blank_retry_after_for_service_unavailable() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 503,
        headers: vec![("Retry-After".to_string(), "   ".to_string())],
        body: b"service unavailable".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new()
        .create(options)
        .expect("client should be created");
    let request = client
        .request(Method::GET, "/service-unavailable-blank-retry-after")
        .build();

    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("response status 503 should fail");

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(error.retry_after, None);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/service-unavailable-blank-retry-after");
}

#[tokio_test]
async fn test_execute_rejects_ipv6_url_when_ipv4_only() {
    let mut options = HttpClientOptions::default();
    options.ipv4_only = true;
    let client = HttpClientBuilder::new()
        .create(options)
        .expect("client should be created");

    let request = client.request(Method::GET, "http://[::1]:18080/reject-ipv6").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("ipv6 request should be rejected");

    assert_eq!(error.kind, HttpErrorKind::InvalidUrl);
    assert!(error.message.contains("IPv6 literal host is not allowed"));
}

#[tokio_test]
async fn test_execute_status_error_preview_reports_body_read_failure() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 503,
        headers: vec![],
        total_length: 64,
        prefix: b"partial-error-body".to_vec(),
        delay: Duration::from_millis(5),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(1);
    let client = HttpClientBuilder::new()
        .create(options)
        .expect("client should be created");
    let request = client.request(Method::GET, "/status-preview-read-error").build();

    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("response status 503 should fail");

    assert_eq!(error.kind, HttpErrorKind::Status);
    let preview = error
        .response_body_preview
        .expect("response body preview should be recorded");
    assert!(preview.contains("failed to read response body"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/status-preview-read-error");
}

#[tokio_test]
async fn test_status_body_preview_truncation_does_not_truncate_raw() {
    let body = br#"{"error":{"code":"quota_exceeded"}}"#.to_vec();
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 429,
        headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
        body: body.clone(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.error_response_preview_limit = 4;
    options.error_response_body_limit = 64;
    let client = HttpClientBuilder::new().create(options).unwrap();
    let error = client
        .execute(client.request(Method::GET, "/quota").build())
        .await
        .unwrap_err();
    assert_ne!(
        error.response_body_preview.as_deref(),
        Some(std::str::from_utf8(&body).unwrap())
    );
    let raw = error.status_response().unwrap();
    assert_eq!(raw.body().as_ref(), body.as_slice());
    assert!(!raw.is_truncated());
    let value: serde_json::Value = serde_json::from_slice(raw.body()).unwrap();
    assert_eq!(value["error"]["code"], "quota_exceeded");
    timeout(Duration::from_secs(3), server.finish()).await.unwrap();
}

#[tokio_test]
async fn test_status_body_raw_limit_is_independent_and_exact_eof_is_complete() {
    for (path, plan, raw_limit, expected_body, expected_truncated) in [
        (
            "/raw-below-limit",
            ResponsePlan::Immediate {
                status: 429,
                headers: vec![],
                body: b"ab".to_vec(),
            },
            4,
            b"ab".as_slice(),
            false,
        ),
        (
            "/raw-known-exact",
            ResponsePlan::Immediate {
                status: 429,
                headers: vec![],
                body: b"abcd".to_vec(),
            },
            4,
            b"abcd".as_slice(),
            false,
        ),
        (
            "/raw-limited",
            ResponsePlan::Immediate {
                status: 429,
                headers: vec![],
                body: b"abcdefgh".to_vec(),
            },
            4,
            b"abcd".as_slice(),
            true,
        ),
        (
            "/chunked-exact",
            ResponsePlan::Chunked {
                status: 429,
                headers: vec![],
                chunks: vec![crate::common::ResponseChunk {
                    delay: Duration::ZERO,
                    bytes: b"abcd".to_vec(),
                }],
                finish: true,
            },
            4,
            b"abcd".as_slice(),
            false,
        ),
    ] {
        let server = spawn_one_shot_server(plan).await;
        let mut options = HttpClientOptions::default();
        options.base_url = Some(server.base_url());
        options.error_response_preview_limit = 64;
        options.error_response_body_limit = raw_limit;
        let client = HttpClientBuilder::new().create(options).unwrap();
        let error = client
            .execute(client.request(Method::GET, path).build())
            .await
            .unwrap_err();
        let raw = error.status_response().unwrap();
        assert_eq!(raw.body().as_ref(), expected_body);
        assert_eq!(raw.is_truncated(), expected_truncated);
        timeout(Duration::from_secs(3), server.finish()).await.unwrap();
    }
}

#[tokio_test]
async fn test_status_body_read_timeout_keeps_partial_raw_truncated() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 429,
        headers: vec![],
        total_length: 64,
        prefix: b"partial".to_vec(),
        delay: Duration::from_millis(200),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_millis(20);
    let client = HttpClientBuilder::new().create(options).unwrap();
    let error = timeout(
        Duration::from_secs(3),
        client.execute(client.request(Method::GET, "/status-body-timeout").build()),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Status);
    assert!(error.response_body_preview.as_ref().unwrap().contains("read timeout"));
    assert!(error.status_response().unwrap().is_truncated());
    timeout(Duration::from_secs(3), server.finish()).await.unwrap();
}

#[tokio_test]
async fn test_status_body_read_timeout_is_cumulative() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (headers_sent_tx, headers_sent_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut byte = [0_u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
        }
        let headers = b"HTTP/1.1 429 Too Many Requests\r\nTransfer-Encoding: chunked\r\nRetry-After: 1\r\nX-Test: preserved\r\nContent-Type: text/plain\r\n\r\n";
        if let Err(error) = stream.write_all(headers).await {
            assert!(matches!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
            ));
            return;
        }
        if let Err(error) = stream.write_all(b"5\r\nfirst\r\n").await {
            assert!(matches!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
            ));
            return;
        }
        let _ = headers_sent_tx.send(());
        for _ in 0..11 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if let Err(error) = stream.write_all(b"1\r\nx\r\n").await {
                assert!(matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                ));
                return;
            }
        }
        let _ = stream.write_all(b"0\r\n\r\n").await;
    });
    let mut options = HttpClientOptions::default();
    options.base_url = Some(url::Url::parse(&format!("http://{address}/")).unwrap());
    options.timeouts.read_timeout = Duration::from_millis(400);
    options.error_response_preview_limit = 256;
    options.error_response_body_limit = 256;
    options.retry.enabled = false;
    let client = HttpClientBuilder::new().create(options).unwrap();
    let request = client.request(Method::GET, "/status-body-cumulative").build();
    let result = tokio::spawn(async move { client.execute(request).await });
    timeout(Duration::from_secs(2), headers_sent_rx)
        .await
        .expect("server did not send response headers")
        .unwrap();
    let error = timeout(Duration::from_secs(3), result)
        .await
        .expect("client execution hung")
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::TOO_MANY_REQUESTS));
    assert_eq!(error.retry_after, Some(Duration::from_secs(1)));
    assert_eq!(error.method, Some(Method::GET));
    assert!(
        error
            .url
            .as_ref()
            .unwrap()
            .as_str()
            .ends_with("/status-body-cumulative")
    );
    let status = error.status_response().unwrap();
    assert_eq!(status.headers().get("x-test").unwrap(), "preserved");
    assert!(status.body().starts_with(b"first"));
    assert!(status.body().len() < b"firstxxxxxxxxxxx".len());
    assert!(status.is_truncated());
    assert!(error.response_body_preview.as_ref().unwrap().contains("read timeout"));
    server.await.unwrap();
}

#[tokio_test]
async fn test_status_body_read_timeout_eof_within_budget_is_complete() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 429,
        headers: vec![],
        chunks: vec![
            crate::common::ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"short ".to_vec(),
            },
            crate::common::ResponseChunk {
                delay: Duration::from_millis(50),
                bytes: b"body".to_vec(),
            },
        ],
        finish: true,
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_millis(400);
    options.error_response_preview_limit = 64;
    options.error_response_body_limit = 64;
    let client = HttpClientBuilder::new().create(options).unwrap();
    let error = timeout(
        Duration::from_secs(3),
        client.execute(client.request(Method::GET, "/status-body-eof").build()),
    )
    .await
    .expect("client execution timed out")
    .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Status);
    let status = error.status_response().unwrap();
    assert_eq!(status.body().as_ref(), b"short body");
    assert!(!status.is_truncated());
    timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
}

#[tokio_test]
async fn test_status_body_cancellation_remains_cancelled() {
    let mut server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 429,
        headers: vec![],
        total_length: 64,
        prefix: b"partial".to_vec(),
        delay: Duration::from_millis(200),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new().create(options).unwrap();
    let token = HttpCancellationToken::new();
    let task_token = token.clone();
    let task = tokio::spawn(async move {
        client
            .execute(
                client
                    .request(Method::GET, "/status-body-cancelled")
                    .cancellation_token(task_token)
                    .build(),
            )
            .await
    });
    server.wait_until_response_started().await;
    token.cancel();
    let error = timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    timeout(Duration::from_secs(3), server.finish()).await.unwrap();
}

#[tokio_test]
async fn test_head_response_ignores_representation_content_length() {
    let server = spawn_one_shot_server(ResponsePlan::ImmediateRawHeaders {
        status: 200,
        headers: vec![("Content-Length".to_string(), b"100".to_vec())],
        body: vec![],
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.response_body_size_limit = 1;
    let client = HttpClientBuilder::new().create(options).unwrap();
    let mut response = client
        .execute(client.request(Method::HEAD, "/head").build())
        .await
        .unwrap();
    assert_eq!(response.headers().get("content-length").unwrap(), "100");
    assert!(response.bytes().await.unwrap().is_empty());
    assert!(response.text().await.unwrap().is_empty());
    let _ = server.finish().await;
}

#[tokio_test]
async fn test_head_status_error_has_complete_empty_raw_body() {
    let server = spawn_one_shot_server(ResponsePlan::ImmediateRawHeaders {
        status: 404,
        headers: vec![("Content-Length".to_string(), b"100".to_vec())],
        body: vec![],
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new().create(options).unwrap();
    let error = client
        .execute(client.request(Method::HEAD, "/head-missing").build())
        .await
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Status);
    let status = error.status_response().unwrap();
    assert!(status.body().is_empty());
    assert!(!status.is_truncated());
    let _ = server.finish().await;
}

#[tokio_test]
async fn test_error_body_capture_returns_at_known_truncation_boundary() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (sent_tx, sent_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut byte = [0_u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
        }
        stream.write_all(b"HTTP/1.1 400 Bad Request\r\nTransfer-Encoding: chunked\r\nContent-Type: text/plain\r\n\r\n5\r\nabcde\r\n").await.unwrap();
        stream.flush().await.unwrap();
        let _ = sent_tx.send(());
        let _ = release_rx.await;
    });
    let mut options = HttpClientOptions::default();
    options.base_url = Some(url::Url::parse(&format!("http://{address}/")).unwrap());
    options.error_response_preview_limit = 4;
    options.error_response_body_limit = 4;
    options.timeouts.read_timeout = Duration::from_secs(30);
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new().create(options).unwrap();
    let result = timeout(
        Duration::from_secs(2),
        client.execute(client.request(Method::GET, "/capture").build()),
    )
    .await;
    sent_rx.await.unwrap();
    let error = result.expect("capture must stop without waiting for EOF").unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Status);
    let captured = error.status_response().unwrap();
    assert_eq!(captured.body().as_ref(), b"abcd");
    assert!(captured.is_truncated());
    assert!(!error.message.contains("body unavailable"));
    let _ = release_tx.send(());
    server.await.unwrap();
}
