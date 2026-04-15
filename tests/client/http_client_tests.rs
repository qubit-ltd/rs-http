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

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use http::header::{HeaderName, AUTHORIZATION, CONTENT_TYPE};
use http::{HeaderValue, Method, StatusCode};
use qubit_http::{
    Delay, HeaderInjector, HttpClientFactory, HttpClientOptions, HttpErrorKind,
    HttpRetryMethodPolicy,
};
use tokio::time::timeout;

use crate::common::{spawn_multi_shot_server, spawn_one_shot_server, ResponseChunk, ResponsePlan};

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
    options.add_header("x-order", "default").unwrap();

    let factory = HttpClientFactory::new();
    let mut client = factory.create_with_options(options).unwrap();
    client.add_header_injector(HeaderInjector::new(|headers| {
        headers.insert(
            HeaderName::from_static("x-order"),
            HeaderValue::from_static("injector"),
        );
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer secret-token"),
        );
        Ok(())
    }));

    let request = client
        .request(Method::POST, "/v1/messages")
        .query_param("stream", "false")
        .header("x-order", "request")
        .unwrap()
        .json_body(&serde_json::json!({"hello":"world"}))
        .unwrap()
        .build();

    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.meta.status, StatusCode::OK);
    let json = response.json::<serde_json::Value>().await.unwrap();
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

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client.request(Method::GET, "/health").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(error.method, Some(Method::GET));
    assert_eq!(
        error.response_body_preview.as_deref(),
        Some("service unavailable")
    );
    assert!(error.message.contains("response body preview"));
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
    let client = HttpClientFactory::new().create().unwrap();
    let request = client.request(Method::GET, "/relative/path").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::InvalidUrl);
}

#[test]
fn test_request_builder_inherits_client_default_options() {
    let mut options = HttpClientOptions::default();
    options
        .set_base_url("https://api.example.com/v1/")
        .expect("base url should be valid");
    options.ipv4_only = true;
    options.timeouts.request_timeout = Some(Duration::from_secs(2));

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .expect("client should be created");
    let request = client.request(Method::GET, "/timeout-default").build();

    assert_eq!(
        request.base_url().map(url::Url::as_str),
        Some("https://api.example.com/v1/")
    );
    assert!(request.ipv4_only());
    assert_eq!(request.request_timeout(), Some(Duration::from_secs(2)));
}

#[test]
fn test_request_builder_methods_override_client_default_options() {
    let mut options = HttpClientOptions::default();
    options
        .set_base_url("https://api.example.com/v1/")
        .expect("base url should be valid");
    options.ipv4_only = true;
    options.timeouts.request_timeout = Some(Duration::from_secs(2));

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .expect("client should be created");

    let request = client
        .request(Method::GET, "/override")
        .base_url(url::Url::parse("https://override.example.com/root/").unwrap())
        .ipv4_only(false)
        .timeout(Duration::from_secs(5))
        .build();

    assert_eq!(
        request.base_url().map(url::Url::as_str),
        Some("https://override.example.com/root/")
    );
    assert!(!request.ipv4_only());
    assert_eq!(request.request_timeout(), Some(Duration::from_secs(5)));
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

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
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

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client.request(Method::GET, "/slow-body").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let error = response.bytes_body().await.unwrap_err();

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
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client.request(Method::GET, "/stream").build();
    let mut stream_response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);
    assert_eq!(
        stream_response.headers().get(CONTENT_TYPE).unwrap(),
        "text/plain"
    );

    let mut body = Vec::new();
    let mut stream = stream_response
        .stream_body()
        .expect("stream body should be available");
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

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client.request(Method::GET, "/stream-timeout").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let mut stream = response
        .stream_body()
        .expect("stream body should be available");

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

#[tokio::test]
async fn test_execute_with_text_body_and_request_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client
        .request(Method::POST, "/text")
        .timeout(Duration::from_secs(1))
        .text_body("hello text")
        .build();

    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.meta.status, StatusCode::OK);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.target, "/text");
    assert_eq!(
        captured.headers.get("content-type").unwrap(),
        "text/plain; charset=utf-8"
    );
    assert_eq!(captured.body, b"hello text");
}

#[tokio::test]
async fn test_execute_with_bytes_body() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client
        .request(Method::PUT, "/bytes")
        .bytes_body(vec![1_u8, 2, 3, 4])
        .build();

    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.meta.status, StatusCode::OK);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.method, "PUT");
    assert_eq!(captured.target, "/bytes");
    assert_eq!(captured.body, vec![1_u8, 2, 3, 4]);
}

#[tokio::test]
async fn test_execute_stream_post_json_body_with_query_and_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        chunks: vec![ResponseChunk {
            delay: Duration::from_millis(0),
            bytes: b"ok".to_vec(),
        }],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client
        .request(Method::POST, "/stream-post")
        .query_param("mode", "events")
        .timeout(Duration::from_secs(1))
        .json_body(&serde_json::json!({"hello":"stream"}))
        .unwrap()
        .build();

    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let body = response
        .stream_body()
        .expect("stream body should be available")
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(body.len(), 1);
    assert_eq!(body[0], b"ok".as_slice());

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.target, "/stream-post?mode=events");
    assert_eq!(
        captured.headers.get("content-type").unwrap(),
        "application/json"
    );
    let json: serde_json::Value = serde_json::from_slice(&captured.body).unwrap();
    assert_eq!(json["hello"], "stream");
}

#[tokio::test]
async fn test_execute_stream_with_text_body() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        chunks: vec![ResponseChunk {
            delay: Duration::from_millis(0),
            bytes: b"ok".to_vec(),
        }],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client
        .request(Method::POST, "/stream-text")
        .text_body("hello stream")
        .build();

    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let body = response
        .stream_body()
        .expect("stream body should be available")
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(body.len(), 1);
    assert_eq!(body[0], b"ok".as_slice());

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.target, "/stream-text");
    assert_eq!(
        captured.headers.get("content-type").unwrap(),
        "text/plain; charset=utf-8"
    );
    assert_eq!(captured.body, b"hello stream");
}

#[tokio::test]
async fn test_execute_stream_with_bytes_body() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![(
            "Content-Type".to_string(),
            "application/octet-stream".to_string(),
        )],
        chunks: vec![ResponseChunk {
            delay: Duration::from_millis(0),
            bytes: b"ok".to_vec(),
        }],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client
        .request(Method::PUT, "/stream-bytes")
        .bytes_body(vec![9_u8, 8, 7])
        .build();

    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let body = response
        .stream_body()
        .expect("stream body should be available")
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(body.len(), 1);
    assert_eq!(body[0], b"ok".as_slice());

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.method, "PUT");
    assert_eq!(captured.target, "/stream-bytes");
    assert_eq!(captured.body, vec![9_u8, 8, 7]);
}

#[tokio::test]
async fn test_execute_stream_maps_non_success_status_to_http_error() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 503,
        headers: vec![],
        body: b"service unavailable".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client.request(Method::GET, "/stream-status").build();

    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(error.method, Some(Method::GET));
    assert_eq!(
        error.response_body_preview.as_deref(),
        Some("service unavailable")
    );
    assert!(error.message.contains("response body preview"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/stream-status");
}

#[tokio::test]
async fn test_execute_non_success_error_body_preview_is_truncated_by_limit() {
    let body = "abcdefghijklmnopqrstuvwxyz";
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 500,
        headers: vec![],
        body: body.as_bytes().to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.error_response_preview_limit = 8;

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client.request(Method::GET, "/status-truncated").build();
    let error = client.execute(request).await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(
        error.response_body_preview.as_deref(),
        Some("abcdefgh...<truncated>")
    );
}

#[tokio::test]
async fn test_execute_non_success_error_body_preview_truncates_when_limit_reached_before_next_chunk(
) {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 500,
        headers: vec![],
        chunks: vec![
            ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"abc".to_vec(),
            },
            ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"def".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.error_response_preview_limit = 3;

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client
        .request(Method::GET, "/status-truncated-next-chunk")
        .build();
    let error = client.execute(request).await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(
        error.response_body_preview.as_deref(),
        Some("abc...<truncated>")
    );
}

#[tokio::test]
async fn test_execute_error_body_preview_limit_is_decoupled_from_logging_limit() {
    let body = "abcdefghijklmnopqrstuvwxyz";
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 500,
        headers: vec![],
        body: body.as_bytes().to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.logging.body_size_limit = 4;
    options.error_response_preview_limit = 12;

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client
        .request(Method::GET, "/status-decoupled-limit")
        .build();
    let error = client.execute(request).await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(
        error.response_body_preview.as_deref(),
        Some("abcdefghijkl...<truncated>")
    );
}

#[tokio::test]
async fn test_execute_non_success_error_body_preview_for_binary_body() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 500,
        headers: vec![],
        body: vec![0_u8, 159, 146, 150, 1, 2],
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.error_response_preview_limit = 16;

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client.request(Method::GET, "/status-binary").build();
    let error = client.execute(request).await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(
        error.response_body_preview.as_deref(),
        Some("<binary 6 bytes>")
    );
}

#[tokio::test]
async fn test_execute_non_success_error_body_preview_for_empty_body() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 500,
        headers: vec![],
        body: vec![],
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client.request(Method::GET, "/status-empty").build();
    let error = client.execute(request).await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.response_body_preview.as_deref(), Some("<empty>"));
}

#[tokio::test]
async fn test_execute_non_success_error_body_preview_timeout_placeholder() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 500,
        headers: vec![],
        total_length: 16,
        prefix: b"abc".to_vec(),
        delay: Duration::from_millis(250),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_millis(30);
    options.timeouts.write_timeout = Duration::from_secs(1);

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client.request(Method::GET, "/status-timeout").build();
    let error = client.execute(request).await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert!(error
        .response_body_preview
        .as_deref()
        .unwrap_or_default()
        .contains("error body unavailable: read timeout"));
}

#[tokio::test]
async fn test_execute_maps_truncated_response_body_to_decode_error() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 200,
        headers: vec![],
        total_length: 8,
        prefix: b"abc".to_vec(),
        delay: Duration::from_millis(0),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client.request(Method::GET, "/truncated-body").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let error = response.bytes_body().await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert_eq!(error.method, Some(Method::GET));
    assert!(error
        .url
        .unwrap()
        .as_str()
        .starts_with(&server.base_url().to_string()));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/truncated-body");
}

#[tokio::test]
async fn test_execute_retries_retryable_status_until_success() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 500,
            headers: vec![],
            body: b"server error".to_vec(),
        },
        ResponsePlan::Immediate {
            status: 200,
            headers: vec![],
            body: b"ok".to_vec(),
        },
    ])
    .await;

    let injector_count = Arc::new(Mutex::new(0_u32));
    let injector_count_clone = injector_count.clone();
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.delay_strategy = Delay::None;
    let mut client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    client.add_header_injector(HeaderInjector::new(move |headers| {
        let mut count = injector_count_clone.lock().unwrap();
        *count += 1;
        headers.insert(
            HeaderName::from_static("x-attempt"),
            HeaderValue::from_str(&count.to_string()).unwrap(),
        );
        Ok(())
    }));

    let request = client.request(Method::GET, "/retry-status").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();

    assert_eq!(response.meta.status, StatusCode::OK);
    assert_eq!(
        response.bytes_body().await.unwrap(),
        Bytes::from_static(b"ok")
    );
    assert_eq!(*injector_count.lock().unwrap(), 2);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].headers.get("x-attempt"), Some(&"1".to_string()));
    assert_eq!(captured[1].headers.get("x-attempt"), Some(&"2".to_string()));
}

#[tokio::test]
async fn test_execute_does_not_retry_non_retryable_status() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 400,
        headers: vec![],
        body: b"bad request".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.delay_strategy = Delay::None;
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client.request(Method::GET, "/bad-request").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::BAD_REQUEST));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/bad-request");
}

#[tokio::test]
async fn test_execute_returns_last_error_after_retry_attempts_exhausted() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 503,
            headers: vec![],
            body: b"unavailable-1".to_vec(),
        },
        ResponsePlan::Immediate {
            status: 503,
            headers: vec![],
            body: b"unavailable-2".to_vec(),
        },
        ResponsePlan::Immediate {
            status: 503,
            headers: vec![],
            body: b"unavailable-3".to_vec(),
        },
    ])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.delay_strategy = Delay::None;
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client.request(Method::GET, "/exhausted").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert!(error.message.contains("retry attempts exhausted: 3/3"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 3);
}

#[tokio::test]
async fn test_execute_retry_max_duration_returns_last_error_after_retry_delay() {
    let server = spawn_multi_shot_server(vec![ResponsePlan::Immediate {
        status: 503,
        headers: vec![],
        body: b"unavailable".to_vec(),
    }])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.max_duration = Some(Duration::from_millis(100));
    options.retry.delay_strategy = Delay::Fixed(Duration::from_millis(120));
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client.request(Method::GET, "/max-duration-after").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert!(error.message.contains("retry max duration exceeded"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 1);
}

#[tokio::test]
async fn test_execute_retry_max_duration_zero_reports_no_retryable_failure() {
    let server = spawn_multi_shot_server(vec![]).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.max_duration = Some(Duration::ZERO);
    options.retry.delay_strategy = Delay::None;
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client.request(Method::GET, "/max-duration-zero").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error
        .message
        .contains("before a retryable error was captured"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert!(captured.is_empty());
}

#[tokio::test]
async fn test_execute_does_not_retry_post_by_default() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 500,
        headers: vec![],
        body: b"server error".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.delay_strategy = Delay::None;
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client.request(Method::POST, "/post-default").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.method, Some(Method::POST));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.method, "POST");
}

#[tokio::test]
async fn test_execute_retries_post_when_all_methods_policy_is_enabled() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 500,
            headers: vec![],
            body: b"server error".to_vec(),
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
    options.retry.delay_strategy = Delay::None;
    options.retry.method_policy = HttpRetryMethodPolicy::AllMethods;
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client.request(Method::POST, "/post-all").build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();

    assert_eq!(response.meta.status, StatusCode::OK);
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].method, "POST");
    assert_eq!(captured[1].method, "POST");
}

#[tokio::test]
async fn test_execute_retries_write_timeout_until_success() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::DelayedStart {
            delay: Duration::from_millis(120),
            status: 200,
            headers: vec![],
            body: b"late".to_vec(),
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
    options.timeouts.write_timeout = Duration::from_millis(30);
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.delay_strategy = Delay::None;
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client.request(Method::GET, "/write-timeout-retry").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();

    assert_eq!(response.meta.status, StatusCode::OK);
    assert_eq!(
        response.bytes_body().await.unwrap(),
        Bytes::from_static(b"ok")
    );
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
}

#[tokio::test]
async fn test_execute_stream_retries_initial_status_until_success() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 503,
            headers: vec![],
            body: b"service unavailable".to_vec(),
        },
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
            chunks: vec![ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"stream-ok".to_vec(),
            }],
            finish: true,
        },
    ])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.delay_strategy = Delay::None;
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client.request(Method::GET, "/stream-retry").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let body = response
        .stream_body()
        .expect("stream body should be available")
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(body[0], b"stream-ok".as_slice());
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
}

#[tokio::test]
async fn test_execute_stream_does_not_retry_after_stream_is_returned() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        chunks: vec![
            ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"first".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_millis(120),
                bytes: b"second".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_millis(30);
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.delay_strategy = Delay::None;
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

    let request = client.request(Method::GET, "/stream-read-timeout").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let mut stream = response
        .stream_body()
        .expect("stream body should be available");
    let first = stream
        .next()
        .await
        .expect("first stream item should exist")
        .unwrap();
    assert_eq!(first, b"first".as_slice());
    let error = stream
        .next()
        .await
        .expect("second stream item should contain read timeout")
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::ReadTimeout);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/stream-read-timeout");
}

#[tokio::test]
async fn test_execute_connect_refused_maps_to_transport_error() {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("ephemeral listener bind should work");
    let addr = listener
        .local_addr()
        .expect("listener should expose a local address");
    drop(listener);

    let client = HttpClientFactory::new()
        .create()
        .expect("default client should be created");
    let target = format!("http://{addr}/refused");
    let request = client.request(Method::GET, &target).build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("closed local port should fail with transport error");

    assert_eq!(error.kind, HttpErrorKind::Transport);
}
