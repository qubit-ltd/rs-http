/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # HttpClient Integration Tests
//!
//! Covers request execution, stream execution, and timeout/error behavior.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use http::header::{HeaderName, AUTHORIZATION, CONTENT_TYPE};
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use qubit_http::{HeaderInjector, HttpClientFactory, HttpClientOptions, HttpErrorKind, HttpResult};
use tokio::time::timeout;

use crate::common::{spawn_one_shot_server, ResponseChunk, ResponsePlan};

#[derive(Debug)]
struct TestHeaderInjector;

impl HeaderInjector for TestHeaderInjector {
    fn inject(&self, headers: &mut HeaderMap) -> HttpResult<()> {
        headers.insert(
            HeaderName::from_static("x-order"),
            HeaderValue::from_static("injector"),
        );
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer secret-token"),
        );
        Ok(())
    }
}

#[tokio::test]
async fn test_execute_success_with_header_injector_and_request_override() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![("Content-Type".to_string(), "application/json".to_string())],
        body: br#"{"ok":true,"value":7}"#.to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.default_headers.insert(
        HeaderName::from_static("x-order"),
        HeaderValue::from_static("default"),
    );

    let factory = HttpClientFactory::new();
    let client = factory.create(options).unwrap();
    client.add_header_injector(Arc::new(TestHeaderInjector));

    let request = client
        .request(Method::POST, "/v1/messages")
        .query_param("stream", "false")
        .header("x-order", "request")
        .unwrap()
        .json_body(&serde_json::json!({"hello":"world"}))
        .unwrap()
        .build();

    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.status, StatusCode::OK);
    let json = response.json::<serde_json::Value>().unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["value"], 7);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.target, "/v1/messages?stream=false");
    assert_eq!(captured.headers.get("x-order").unwrap(), "request");
    assert_eq!(
        captured.headers.get("authorization").unwrap(),
        "Bearer secret-token"
    );
    assert_eq!(
        captured.headers.get("content-type").unwrap(),
        "application/json"
    );

    assert!(captured.headers.contains_key("content-length"));
}

#[tokio::test]
async fn test_execute_maps_non_success_status_to_http_error() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 503,
        headers: vec![],
        body: b"service unavailable".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client.request(Method::GET, "/health").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(error.method, Some(Method::GET));
    assert!(error
        .url
        .unwrap()
        .as_str()
        .starts_with(&server.base_url().to_string()));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.method, "GET");
    assert_eq!(captured.target, "/health");
}

#[tokio::test]
async fn test_execute_relative_path_without_base_url_returns_invalid_url() {
    let client = HttpClientFactory::new()
        .create(HttpClientOptions::default())
        .unwrap();
    let request = client.request(Method::GET, "/relative/path").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::InvalidUrl);
}

#[tokio::test]
async fn test_execute_write_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_millis(250),
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_millis(80);
    options.timeouts.read_timeout = Duration::from_secs(1);

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client.request(Method::GET, "/delayed").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::WriteTimeout);
    assert_eq!(error.method, Some(Method::GET));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/delayed");
}

#[tokio::test]
async fn test_execute_read_timeout_on_buffered_body() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        total_length: 8,
        prefix: b"abc".to_vec(),
        delay: Duration::from_millis(250),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_secs(1);
    options.timeouts.read_timeout = Duration::from_millis(80);

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client.request(Method::GET, "/slow-body").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::ReadTimeout);
    assert_eq!(error.method, Some(Method::GET));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/slow-body");
}

#[tokio::test]
async fn test_execute_stream_success_reads_all_chunks() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        chunks: vec![
            ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"hello ".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"world".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client.request(Method::GET, "/stream").build();
    let stream_response = timeout(Duration::from_secs(3), client.execute_stream(request))
        .await
        .expect("execute_stream timed out")
        .unwrap();
    assert_eq!(stream_response.status, StatusCode::OK);
    assert_eq!(
        stream_response.headers.get(CONTENT_TYPE).unwrap(),
        "text/plain"
    );

    let mut body = Vec::new();
    let mut stream = stream_response.into_stream();
    while let Some(item) = stream.next().await {
        let bytes = item.unwrap();
        body.extend_from_slice(&bytes);
    }
    assert_eq!(body, b"hello world");

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/stream");
}

#[tokio::test]
async fn test_execute_stream_read_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        chunks: vec![
            ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"first".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_millis(250),
                bytes: b"second".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_millis(80);
    options.timeouts.write_timeout = Duration::from_secs(1);

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client.request(Method::GET, "/stream-timeout").build();
    let response = timeout(Duration::from_secs(3), client.execute_stream(request))
        .await
        .expect("execute_stream timed out")
        .unwrap();
    let mut stream = response.into_stream();

    let first = stream.next().await.unwrap().unwrap();
    assert_eq!(first, b"first".as_slice());

    let second = stream.next().await.unwrap();
    let error = second.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::ReadTimeout);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/stream-timeout");
}
