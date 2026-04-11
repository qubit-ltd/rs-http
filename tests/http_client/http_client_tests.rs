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

use std::time::Duration;

use futures_util::StreamExt;
use http::header::{HeaderName, AUTHORIZATION, CONTENT_TYPE};
use http::{HeaderValue, Method, StatusCode};
use qubit_http::{HeaderInjector, HttpClientFactory, HttpClientOptions, HttpErrorKind};
use tokio::time::timeout;

use crate::common::{spawn_one_shot_server, ResponseChunk, ResponsePlan};

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
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();

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

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
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
    assert_eq!(response.status, StatusCode::OK);

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
    assert_eq!(response.status, StatusCode::OK);

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

    let response = timeout(Duration::from_secs(3), client.execute_stream(request))
        .await
        .expect("execute_stream timed out")
        .unwrap();
    let body = response
        .into_stream()
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

    let response = timeout(Duration::from_secs(3), client.execute_stream(request))
        .await
        .expect("execute_stream timed out")
        .unwrap();
    let body = response
        .into_stream()
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

    let response = timeout(Duration::from_secs(3), client.execute_stream(request))
        .await
        .expect("execute_stream timed out")
        .unwrap();
    let body = response
        .into_stream()
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

    let error = timeout(Duration::from_secs(3), client.execute_stream(request))
        .await
        .expect("execute_stream timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(error.method, Some(Method::GET));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/stream-status");
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

    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

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
