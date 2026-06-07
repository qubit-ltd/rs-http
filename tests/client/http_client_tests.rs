// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # HttpClient Integration Tests
//!
//! Covers request execution, stream execution, and timeout/error behavior.

use std::error::Error as StdError;
use std::sync::{
    Arc,
    Mutex,
};
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use http::header::{
    HeaderName,
    AUTHORIZATION,
    CONTENT_TYPE,
};
use http::{
    HeaderValue,
    Method,
    StatusCode,
};
use qubit_http::{
    HttpClientFactory,
    HttpClientOptions,
    HttpError,
    HttpErrorKind,
    HttpHeaderInjector,
    HttpResponseInterceptor,
    HttpRetryMethodPolicy,
    RetryDelay,
};
use qubit_sanitize::SensitivityLevel;
use tokio::time::timeout;

use crate::common::{
    spawn_multi_shot_server,
    spawn_one_shot_server,
    ResponseChunk,
    ResponsePlan,
};

fn retry_abort_inner_http(error: &HttpError) -> &HttpError {
    let boxed = error
        .source
        .as_ref()
        .expect("retry abort should chain inner error");
    (boxed.as_ref() as &(dyn StdError + 'static))
        .downcast_ref::<HttpError>()
        .expect("inner should be HttpError")
}

#[tokio::test]
async fn test_execute_success_with_header_injector_and_request_override() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![(
            "Content-Type".to_string(),
            "application/json".to_string(),
        )],
        body: br#"{"ok":true,"value":7}"#.to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.add_header("x-order", "default").unwrap();

    let factory = HttpClientFactory::new();
    let mut client = factory.create(options).unwrap();
    client.add_header_injector(HttpHeaderInjector::new(|headers| {
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
    assert_eq!(response.status(), StatusCode::OK);
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

    let client = HttpClientFactory::new().create(options).unwrap();
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
        Some("<redacted: unsupported HTTP body>")
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
    let client = HttpClientFactory::new().create_default().unwrap();
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
        .create(options)
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
        .create(options)
        .expect("client should be created");

    let request = client
        .request(Method::GET, "/override")
        .base_url(
            url::Url::parse("https://override.example.com/root/").unwrap(),
        )
        .ipv4_only(false)
        .request_timeout(Duration::from_secs(5))
        .expect("positive request timeout should be accepted")
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
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let error = response.bytes().await.unwrap_err();

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
    let mut stream_response =
        timeout(Duration::from_secs(3), client.execute(request))
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
        .stream()
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

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client.request(Method::GET, "/stream-timeout").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let mut stream =
        response.stream().expect("stream body should be available");

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
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client
        .request(Method::POST, "/text")
        .request_timeout(Duration::from_secs(1))
        .expect("positive request timeout should be accepted")
        .text_body("hello text")
        .build();

    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

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
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client
        .request(Method::PUT, "/bytes")
        .bytes_body(vec![1_u8, 2, 3, 4])
        .build();

    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

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
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client
        .request(Method::POST, "/stream-post")
        .query_param("mode", "events")
        .request_timeout(Duration::from_secs(1))
        .expect("positive request timeout should be accepted")
        .json_body(&serde_json::json!({"hello":"stream"}))
        .unwrap()
        .build();

    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let body = response
        .stream()
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
    let json: serde_json::Value =
        serde_json::from_slice(&captured.body).unwrap();
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
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client
        .request(Method::POST, "/stream-text")
        .text_body("hello stream")
        .build();

    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let body = response
        .stream()
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
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client
        .request(Method::PUT, "/stream-bytes")
        .bytes_body(vec![9_u8, 8, 7])
        .build();

    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let body = response
        .stream()
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
    let client = HttpClientFactory::new().create(options).unwrap();
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
        Some("<redacted: unsupported HTTP body>")
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

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client.request(Method::GET, "/status-truncated").build();
    let error = client.execute(request).await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(
        error.response_body_preview.as_deref(),
        Some("<redacted: unsupported HTTP body>...<truncated>")
    );
}

#[tokio::test]
async fn test_execute_non_success_error_body_preview_is_not_truncated_at_exact_limit(
) {
    let body = "abcdefgh";
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 500,
        headers: vec![],
        body: body.as_bytes().to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.error_response_preview_limit = body.len();

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client
        .request(Method::GET, "/status-exact-preview-limit")
        .build();
    let error = client.execute(request).await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(
        error.response_body_preview.as_deref(),
        Some("<redacted: unsupported HTTP body>")
    );
}

#[tokio::test]
async fn test_execute_response_metadata_debug_uses_custom_log_policy() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![(
            "x-tenant-secret".to_string(),
            "custom-header-secret".to_string(),
        )],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options
        .log_sanitize_policy
        .sensitive_headers
        .insert("x-tenant-secret", SensitivityLevel::High);
    options
        .log_sanitize_policy
        .sensitive_query_params
        .insert("tenant_marker", SensitivityLevel::High);

    let captured_context_debug = Arc::new(Mutex::new(None));
    let captured_context_debug_for_interceptor =
        Arc::clone(&captured_context_debug);
    let mut client = HttpClientFactory::new().create(options).unwrap();
    client.add_response_interceptor(HttpResponseInterceptor::new(
        move |context| {
            *captured_context_debug_for_interceptor
                .lock()
                .expect("lock captured context debug") =
                Some(format!("{context:?}"));
            Ok(())
        },
    ));

    let request = client
        .request(
            Method::GET,
            "/status-custom-policy?tenant_marker=custom-query-secret",
        )
        .build();
    let response = client.execute(request).await.unwrap();
    let meta_debug = format!("{:?}", response.meta());
    let context_debug = captured_context_debug
        .lock()
        .expect("lock captured context debug for assertion")
        .clone()
        .expect("response interceptor should capture debug output");

    for debug in [&meta_debug, &context_debug] {
        assert!(!debug.contains("custom-header-secret"));
        assert!(!debug.contains("custom-query-secret"));
        assert!(debug.contains("****"));
    }
}

#[tokio::test]
async fn test_execute_non_success_error_body_preview_sanitizes_json_fields() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 400,
        headers: vec![(
            "Content-Type".to_string(),
            "application/json".to_string(),
        )],
        body: br#"{"user":"alice","password":"secret"}"#.to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client
        .request(Method::GET, "/status-sensitive-body")
        .build();
    let error = client.execute(request).await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(
        error.response_body_preview.as_deref(),
        Some(r#"{"password":"<redacted>","user":"alice"}"#)
    );
    assert!(!error.message.contains("secret"));
}

#[tokio::test]
async fn test_execute_status_error_message_sanitizes_sensitive_url_parts() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 401,
        headers: vec![],
        body: b"unauthorized".to_vec(),
    })
    .await;
    let mut url = server
        .base_url()
        .join("/status-sensitive-url?accessToken=query-secret")
        .expect("request URL should join");
    url.set_username("debug-user")
        .expect("URL should accept username");
    url.set_password(Some("url-secret"))
        .expect("URL should accept password");
    url.set_fragment(Some("fragment-secret"));

    let client = HttpClientFactory::new().create_default().unwrap();
    let request = client.request(Method::GET, url.as_str()).build();
    let error = client.execute(request).await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert!(!error.message.contains("debug-user"));
    assert!(!error.message.contains("url-secret"));
    assert!(!error.message.contains("query-secret"));
    assert!(!error.message.contains("fragment-secret"));
    assert!(error.message.contains("****"));

    let captured = server.finish().await;
    assert_eq!(
        captured.target,
        "/status-sensitive-url?accessToken=query-secret"
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

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client
        .request(Method::GET, "/status-truncated-next-chunk")
        .build();
    let error = client.execute(request).await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(
        error.response_body_preview.as_deref(),
        Some("<redacted: unsupported HTTP body>...<truncated>")
    );
}

#[tokio::test]
async fn test_execute_error_body_preview_limit_is_decoupled_from_logging_limit()
{
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

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client
        .request(Method::GET, "/status-decoupled-limit")
        .build();
    let error = client.execute(request).await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(
        error.response_body_preview.as_deref(),
        Some("<redacted: unsupported HTTP body>...<truncated>")
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

    let client = HttpClientFactory::new().create(options).unwrap();
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

    let client = HttpClientFactory::new().create(options).unwrap();
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

    let client = HttpClientFactory::new().create(options).unwrap();
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
async fn test_execute_maps_truncated_response_body_to_transport_error() {
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
    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client.request(Method::GET, "/truncated-body").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let error = response.bytes().await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Transport);
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
async fn test_execute_maps_truncated_response_stream_to_transport_error() {
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
    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client.request(Method::GET, "/truncated-stream").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let mut stream =
        response.stream().expect("stream body should be available");

    let first = stream
        .next()
        .await
        .expect("first stream item should exist")
        .expect("first stream item should be bytes");
    assert_eq!(first, b"abc".as_slice());
    let error = stream
        .next()
        .await
        .expect("second stream item should contain read error")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Transport);
    assert_eq!(error.method, Some(Method::GET));
    assert!(error
        .url
        .unwrap()
        .as_str()
        .starts_with(&server.base_url().to_string()));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/truncated-stream");
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
    options.retry.delay_strategy = RetryDelay::None;
    let mut client = HttpClientFactory::new().create(options).unwrap();
    client.add_header_injector(HttpHeaderInjector::new(move |headers| {
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

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.bytes().await.unwrap(), Bytes::from_static(b"ok"));
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
    options.retry.delay_strategy = RetryDelay::None;
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client.request(Method::GET, "/bad-request").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::RetryAborted);
    let inner = retry_abort_inner_http(&error);
    assert_eq!(inner.kind, HttpErrorKind::Status);
    assert_eq!(inner.status, Some(StatusCode::BAD_REQUEST));

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
    options.retry.delay_strategy = RetryDelay::None;
    let client = HttpClientFactory::new().create(options).unwrap();

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
async fn test_execute_retry_max_duration_returns_last_error_after_retry_delay()
{
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
    options.retry.delay_strategy =
        RetryDelay::Fixed(Duration::from_millis(120));
    let client = HttpClientFactory::new().create(options).unwrap();

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
    options.retry.delay_strategy = RetryDelay::None;
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client.request(Method::GET, "/max-duration-zero").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::RetryMaxElapsedExceeded);
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
    options.retry.delay_strategy = RetryDelay::None;
    let client = HttpClientFactory::new().create(options).unwrap();

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
    options.retry.delay_strategy = RetryDelay::None;
    options.retry.method_policy = HttpRetryMethodPolicy::AllMethods;
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client.request(Method::POST, "/post-all").build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
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
    options.retry.delay_strategy = RetryDelay::None;
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client.request(Method::GET, "/write-timeout-retry").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.bytes().await.unwrap(), Bytes::from_static(b"ok"));
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
            headers: vec![(
                "Content-Type".to_string(),
                "text/plain".to_string(),
            )],
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
    options.retry.delay_strategy = RetryDelay::None;
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client.request(Method::GET, "/stream-retry").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let body = response
        .stream()
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
    options.retry.delay_strategy = RetryDelay::None;
    let client = HttpClientFactory::new().create(options).unwrap();

    let request = client.request(Method::GET, "/stream-read-timeout").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    let mut stream =
        response.stream().expect("stream body should be available");
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
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("ephemeral listener bind should work");
    let addr = listener
        .local_addr()
        .expect("listener should expose a local address");
    drop(listener);

    let client = HttpClientFactory::new()
        .create_default()
        .expect("default client should be created");
    let target = format!("http://{addr}/refused");
    let request = client.request(Method::GET, &target).build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("closed local port should fail with transport error");

    assert_eq!(error.kind, HttpErrorKind::Transport);
}
