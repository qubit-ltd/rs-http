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

use http::Method;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::test as tokio_test;
use tokio::time::timeout;

use crate::common::ResponsePlan;
use crate::common::spawn_one_shot_server;

async fn spawn_tls_stall_server() -> (String, Arc<AtomicUsize>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind TLS stall listener");
    let address = listener.local_addr().expect("read listener address");
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed_attempts = Arc::clone(&attempts);
    let task = tokio::spawn(async move {
        let mut streams = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            observed_attempts.fetch_add(1, Ordering::SeqCst);
            // Keep TCP connections open without sending a TLS ServerHello.
            streams.push(stream);
        }
        drop(streams);
    });
    (format!("https://{address}/"), attempts, task)
}

#[tokio_test]
async fn test_backend_error_mapper_classifies_tls_handshake_timeout_as_connect_timeout() {
    let (url, attempts, listener_task) = spawn_tls_stall_server().await;
    let mut options = HttpClientOptions::default();
    options.use_env_proxy = false;
    options.timeouts.connect_timeout = Duration::from_millis(100);
    options.timeouts.request_timeout = Some(Duration::from_secs(2));
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.retry_error_kinds = Some(vec![HttpErrorKind::ConnectTimeout]);
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let error = timeout(
        Duration::from_secs(5),
        client.execute(client.request(Method::GET, &url).build()),
    )
    .await
    .expect("request should remain bounded")
    .expect_err("TLS handshake should stall");

    assert_eq!(error.kind, HttpErrorKind::ConnectTimeout);
    assert!(error.source.is_some(), "underlying reqwest error should be retained");
    assert_eq!(error.method, Some(Method::GET));
    assert_eq!(error.url.as_ref().map(url::Url::as_str), Some(url.as_str()));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);

    listener_task.abort();
    let _ = listener_task.await;
}

#[tokio_test]
async fn test_backend_error_mapper_keeps_connection_refusal_as_transport() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
    let address = listener.local_addr().expect("read listener address");
    drop(listener);

    let mut options = HttpClientOptions::default();
    options.use_env_proxy = false;
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");
    let url = format!("http://{address}/refused");
    let error = client
        .execute(client.request(Method::GET, &url).build())
        .await
        .expect_err("closed port should refuse the connection");

    assert_eq!(error.kind, HttpErrorKind::Transport);
}

#[tokio_test]
async fn test_backend_error_mapper_classifies_request_timeout_separately() {
    let server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_secs(2),
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.use_env_proxy = false;
    options.base_url = Some(server.base_url());
    options.timeouts.request_timeout = Some(Duration::from_millis(25));
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");
    let error = client
        .execute(client.request(Method::GET, "/request-timeout").build())
        .await
        .expect_err("delayed response should exceed request timeout");

    assert_eq!(error.kind, HttpErrorKind::RequestTimeout);
    timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
}

#[tokio_test]
async fn test_backend_error_mapper_classifies_body_read_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 200,
        headers: vec![],
        total_length: 16,
        prefix: b"partial".to_vec(),
        delay: Duration::from_secs(2),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_millis(25);
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");
    let mut response = client
        .execute(client.request(Method::GET, "/read-timeout").build())
        .await
        .expect("headers should be received before body timeout");

    let error = response
        .bytes()
        .await
        .expect_err("delayed body should exceed read timeout");

    assert_eq!(error.kind, HttpErrorKind::ReadTimeout);
    assert_eq!(error.method, Some(Method::GET));
    assert_eq!(error.url, Some(server.base_url().join("/read-timeout").unwrap()));

    timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
}
