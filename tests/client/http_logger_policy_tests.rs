// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use bytes::Bytes;
use futures_util::stream;
use futures_util::StreamExt;
use http::header::AUTHORIZATION;
use http::header::CONTENT_TYPE;
use http::header::SET_COOKIE;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use http::StatusCode;
use qubit_http::sse::SseJsonMode;
use qubit_http::sse::SseReconnectOptions;
use qubit_http::HttpCancellationToken;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpError;
use qubit_http::HttpErrorKind;
use qubit_http::HttpLogger;
use qubit_http::HttpLoggingOptions;
use qubit_http::HttpRequest;
use qubit_http::HttpRequestBody;
use qubit_http::HttpRequestBodyByteStream;
use qubit_http::HttpRequestInterceptor;
use qubit_http::HttpResponse;
use qubit_http::HttpResponseMeta;
use qubit_redact::formats::http::TextBodyPolicy;
use qubit_redact::RedactionPolicy;
use qubit_retry::BackoffPolicy;
use qubit_retry::RetryError;
use tokio::runtime::Builder;
use tokio::time::timeout;
use url::Url;

use crate::common::capture_trace_logs;
use crate::common::spawn_multi_shot_server;
use crate::common::spawn_one_shot_server;
use crate::common::ResponseChunk;
use crate::common::ResponsePlan;

fn logging_request(
    method: Method,
    path: &str,
    headers: HeaderMap,
    body: HttpRequestBody,
) -> HttpRequest {
    let client = HttpClientBuilder::new()
        .build()
        .expect("default options should create client");
    let base = client.request(method, path).headers(headers);
    match body {
        HttpRequestBody::Empty => base.build(),
        HttpRequestBody::Text(text) => base.text_body(text).build(),
        HttpRequestBody::Json(bytes) | HttpRequestBody::Bytes(bytes) => {
            base.bytes_body(bytes).build()
        }
        HttpRequestBody::Stream(chunks) => base.stream_body(chunks).build(),
        other => {
            panic!("logging_request: unsupported body variant: {:?}", other)
        }
    }
}

#[test]
fn test_log_request_disabled_emits_nothing() {
    let mut options = HttpLoggingOptions::default();
    options.enabled = false;
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    let logger = HttpLogger::new(&client_options);

    let request = logging_request(
        Method::POST,
        "https://example.com/api",
        headers,
        HttpRequestBody::Json(Bytes::from_static(br#"{"x":1}"#)),
    );
    let logs = capture_trace_logs(|| {
        logger.log_request(&request);
    });
    assert!(logs.trim().is_empty());
}

#[test]
fn test_log_request_toggles_header_and_body() {
    let mut options = HttpLoggingOptions::default();
    options.log_request_header = false;
    options.log_request_body = false;

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    let logger = HttpLogger::new(&client_options);

    let request = logging_request(
        Method::POST,
        "https://example.com/api",
        headers,
        HttpRequestBody::Json(Bytes::from_static(br#"{"x":1}"#)),
    );
    let logs = capture_trace_logs(|| {
        logger.log_request(&request);
    });
    assert!(logs.contains("--> POST https://example.com/api"));
    assert!(!logs.contains("application/json"));
    assert!(!logs.contains("Request body:"));
}

#[test]
fn test_log_response_masks_sensitive_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, HeaderValue::from_static("session-token-value"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer very-secret-token"),
    );
    let options = HttpLoggingOptions::default();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    let logger = HttpLogger::new(&client_options);

    let logs = capture_trace_logs(|| {
        let mut response = HttpResponse::new(
            StatusCode::OK,
            headers.clone(),
            Bytes::from_static(b"ok"),
            Url::parse("https://example.com/data").expect("response URL should parse"),
            Method::GET,
        );
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime
            .block_on(async { logger.log_response(&mut response).await })
            .expect("response logging should succeed");
    });
    assert!(logs.contains("set-cookie: [****]"));
    assert!(logs.contains("authorization: [****]"));
}

#[test]
fn test_log_response_masks_sensitive_response_url() {
    let options = HttpLoggingOptions::default();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    let logger = HttpLogger::new(&client_options);

    let logs = capture_trace_logs(|| {
        let mut response = HttpResponse::new(
            StatusCode::OK,
            HeaderMap::new(),
            Bytes::from_static(b"ok"),
            Url::parse("https://example.com/data?password=secret&access_token=raw-access-secret")
                .expect("response URL should parse"),
            Method::GET,
        );
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime
            .block_on(async { logger.log_response(&mut response).await })
            .expect("response logging should succeed");
    });

    assert!(
        logs.contains("<-- 200 https://example.com/data?password=%3Credacted%3E&access_token=****")
    );
    assert!(!logs.contains("secret"));
}

#[test]
fn test_log_stream_response_headers_masks_sensitive_response_url() {
    let options = HttpLoggingOptions::default();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    let logger = HttpLogger::new(&client_options);
    let response_meta = HttpResponseMeta::new(
        StatusCode::OK,
        HeaderMap::new(),
        Url::parse("https://example.com/stream?password=secret&access_token=raw-access-secret")
            .expect("response URL should parse"),
        Method::GET,
    );

    let logs = capture_trace_logs(|| {
        logger.log_stream_response_headers(&response_meta);
    });

    assert!(logs.contains(
        "<-- 200 https://example.com/stream?password=%3Credacted%3E&access_token=**** (stream)"
    ));
    assert!(!logs.contains("secret"));
}

#[test]
fn test_log_response_binary_body_and_truncation() {
    let options = HttpLoggingOptions {
        body_size_limit: 4,
        ..HttpLoggingOptions::default()
    };
    let headers = HeaderMap::new();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    let logger = HttpLogger::new(&client_options);

    let logs = capture_trace_logs(|| {
        let mut response = HttpResponse::new(
            StatusCode::OK,
            headers.clone(),
            Bytes::from_static(&[0xFF, 0xFE, 0xFD, 0xFC, 0xFB]),
            Url::parse("https://example.com/bin").expect("response URL should parse"),
            Method::GET,
        );
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime
            .block_on(async { logger.log_response(&mut response).await })
            .expect("response logging should succeed");
    });
    assert!(logs.contains("Response body: <redaction incomplete>"));
}

#[test]
fn test_log_stream_response_headers_respects_toggle() {
    let mut options = HttpLoggingOptions::default();
    options.log_response_header = false;
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    let logger = HttpLogger::new(&client_options);

    let logs = capture_trace_logs(|| {
        let response_meta = HttpResponseMeta::new(
            StatusCode::OK,
            headers.clone(),
            Url::parse("https://example.com/stream").expect("response URL should parse"),
            Method::GET,
        );
        logger.log_stream_response_headers(&response_meta);
    });
    assert!(logs.contains("<-- 200 https://example.com/stream (stream)"));
    assert!(!logs.contains("text/event-stream"));
}

#[test]
fn test_log_request_includes_builder_query_params() {
    let client = HttpClientBuilder::new()
        .build()
        .expect("default options should create client");
    let logger_options = HttpClientOptions::default();
    let logger = HttpLogger::new(&logger_options);
    let request = client
        .request(Method::GET, "https://example.com/api?existing=1")
        .query_param("added", "two words")
        .build();

    let logs = capture_trace_logs(|| {
        logger.log_request(&request);
    });

    assert!(logs.contains("--> GET https://example.com/api?existing=1&added=two+words"));
}

#[test]
fn test_log_request_text_body() {
    let options = HttpLoggingOptions::default();
    let headers = HeaderMap::new();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    let mut policy_builder = RedactionPolicy::default().to_builder();
    policy_builder = policy_builder
        .http(|http| {
            http.text_body(TextBodyPolicy::PassThrough);
        })
        .expect("test policy should be valid");
    client_options.log_redaction_policy = policy_builder
        .build()
        .expect("log redaction policy should be valid");
    let logger = HttpLogger::new(&client_options);

    let request = logging_request(
        Method::POST,
        "https://example.com/text",
        headers,
        HttpRequestBody::Text("hello body".to_string()),
    );
    let logs = capture_trace_logs(|| {
        logger.log_request(&request);
    });
    assert!(logs.contains("--> POST https://example.com/text"));
    assert!(logs.contains("Request body: hello body"));
}

#[test]
fn test_log_request_text_and_retry_bytes_share_content_type_and_redacted_preview() {
    let logger_options = HttpClientOptions::default();
    let logger = HttpLogger::new(&logger_options);
    let text_request = logging_request(
        Method::POST,
        "https://example.com/text-retry",
        HeaderMap::new(),
        HttpRequestBody::Text("trace-shared-text-secret".to_string()),
    );
    let mut bytes_headers = HeaderMap::new();
    bytes_headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    let bytes_request = logging_request(
        Method::POST,
        "https://example.com/text-retry",
        bytes_headers,
        HttpRequestBody::Bytes(Bytes::from_static(b"trace-shared-text-secret")),
    );

    assert_eq!(
        text_request.headers().get(CONTENT_TYPE),
        bytes_request.headers().get(CONTENT_TYPE)
    );
    let text_logs = capture_trace_logs(|| logger.log_request(&text_request));
    let bytes_logs = capture_trace_logs(|| logger.log_request(&bytes_request));
    let body_preview = |logs: &str| {
        logs.lines()
            .find(|line| line.contains("Request body:"))
            .expect("request body preview should be logged")
            .to_owned()
    };
    assert_eq!(body_preview(&text_logs), body_preview(&bytes_logs));
    assert!(text_logs.contains("<redacted: text body>"));
    assert!(!text_logs.contains("trace-shared-text-secret"));
    assert!(!bytes_logs.contains("trace-shared-text-secret"));
}

#[test]
fn test_log_request_text_body_redacts_by_default() {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/plain"));
    let client_options = HttpClientOptions::default();
    let logger = HttpLogger::new(&client_options);
    let request = logging_request(
        Method::POST,
        "https://example.com/text",
        headers,
        HttpRequestBody::Text("trace-request-secret".to_string()),
    );

    let logs = capture_trace_logs(|| {
        logger.log_request(&request);
    });

    assert!(logs.contains("Request body: <redacted: text body>"));
    assert!(!logs.contains("trace-request-secret"));
}

#[test]
fn test_log_request_non_utf8_content_type_redacts_body() {
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_bytes(b"application/json; charset=\xFF")
            .expect("non-UTF-8 header value should be accepted"),
    );
    let client_options = HttpClientOptions::default();
    let logger = HttpLogger::new(&client_options);
    let request = logging_request(
        Method::POST,
        "https://example.com/non-utf8-content-type",
        headers,
        HttpRequestBody::Json(Bytes::from_static(
            br#"{"note":"unclassified-request-secret"}"#,
        )),
    );

    let logs = capture_trace_logs(|| {
        logger.log_request(&request);
    });

    assert!(logs.contains("Request body: <redacted: invalid content type body>"));
    assert!(!logs.contains("unclassified-request-secret"));
}

#[test]
fn test_log_response_text_body_redacts_by_default() {
    let client_options = HttpClientOptions::default();
    let logger = HttpLogger::new(&client_options);
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/plain"));

    let logs = capture_trace_logs(|| {
        let mut response = HttpResponse::new(
            StatusCode::OK,
            headers,
            Bytes::from_static(b"trace-response-secret"),
            Url::parse("https://example.com/text").expect("response URL should parse"),
            Method::GET,
        );
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime
            .block_on(async { logger.log_response(&mut response).await })
            .expect("response logging should succeed");
    });

    assert!(logs.contains("Response body: <redacted: text body>"));
    assert!(!logs.contains("trace-response-secret"));
}

#[test]
fn test_log_request_stream_body_logged_as_skipped() {
    let options = HttpLoggingOptions::default();
    let headers = HeaderMap::new();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    let logger = HttpLogger::new(&client_options);

    let request = logging_request(
        Method::POST,
        "https://example.com/stream-upload",
        headers,
        HttpRequestBody::Stream(vec![Bytes::from_static(b"a"), Bytes::from_static(b"b")]),
    );
    let logs = capture_trace_logs(|| {
        logger.log_request(&request);
    });
    assert!(logs.contains("--> POST https://example.com/stream-upload"));
    assert!(logs.contains("Request body: <skipped: streaming request body>"));
}

#[test]
fn test_log_request_streaming_body_logged_as_skipped() {
    let options = HttpLoggingOptions::default();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    let logger = HttpLogger::new(&client_options);
    let client = HttpClientBuilder::new()
        .build()
        .expect("default options should create client");

    let request = client
        .request(Method::POST, "https://example.com/streaming-upload")
        .streaming_body(|| {
            Box::pin(async move {
                Box::pin(stream::iter([Ok(Bytes::from_static(b"secret-stream"))]))
                    as HttpRequestBodyByteStream
            })
        })
        .build();
    let logs = capture_trace_logs(|| {
        logger.log_request(&request);
    });
    assert!(logs.contains("--> POST https://example.com/streaming-upload"));
    assert!(logs.contains("Request body: <skipped: streaming request body>"));
    assert!(!logs.contains("secret-stream"));
}

#[test]
fn test_log_request_hides_raw_path_when_url_resolution_fails() {
    let client_options = HttpClientOptions::default();
    let logger = HttpLogger::new(&client_options);
    let request = logging_request(
        Method::GET,
        "/relative-only?access_token=secret-token",
        HeaderMap::new(),
        HttpRequestBody::Empty,
    );

    let logs = capture_trace_logs(|| {
        logger.log_request(&request);
    });

    assert!(logs.contains("--> GET <unresolved request URL>"));
    assert!(!logs.contains("relative-only"));
    assert!(!logs.contains("secret-token"));
}

#[test]
fn test_execute_returns_body_read_error_from_response_logging() {
    let logs = capture_trace_logs(|| {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime.block_on(async {
            let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
                status: 200,
                headers: vec![],
                total_length: 64,
                prefix: b"partial-body".to_vec(),
                delay: std::time::Duration::from_millis(5),
            })
            .await;

            let mut options = HttpClientOptions::default();
            options.base_url = Some(server.base_url());
            options.timeouts.read_timeout = std::time::Duration::from_secs(1);
            let client = HttpClientBuilder::new()
                .options(options)
                .build()
                .expect("client should be created");

            let request = client
                .request(Method::GET, "/trace-body-read-error")
                .build();
            let response = timeout(std::time::Duration::from_secs(3), client.execute(request))
                .await
                .expect("execute timed out")
                .expect("response logging must not consume the body");
            drop(response);

            let captured = timeout(std::time::Duration::from_secs(3), server.finish())
                .await
                .expect("server finish timed out");
            assert_eq!(captured.target, "/trace-body-read-error");
        });
    });
    assert!(logs.contains("<-- 200"));
}

#[test]
fn test_execute_skips_trace_response_body_for_streaming_or_unknown_size_body() {
    let logs = capture_trace_logs(|| {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime.block_on(async {
            let server = spawn_one_shot_server(ResponsePlan::Chunked {
                status: 200,
                headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
                chunks: vec![
                    ResponseChunk {
                        delay: std::time::Duration::ZERO,
                        bytes: b"first".to_vec(),
                    },
                    ResponseChunk {
                        delay: std::time::Duration::from_millis(250),
                        bytes: b"second".to_vec(),
                    },
                ],
                finish: true,
            })
            .await;

            let mut options = HttpClientOptions::default();
            options.base_url = Some(server.base_url());
            options.timeouts.read_timeout = std::time::Duration::from_millis(80);
            let client = HttpClientBuilder::new()
                .options(options)
                .build()
                .expect("client should be created");

            let request = client.request(Method::GET, "/trace-streaming").build();
            let mut response = timeout(std::time::Duration::from_secs(3), client.execute(request))
                .await
                .expect("execute timed out")
                .expect("request should start");
            let mut stream = response.stream().expect("stream body should be available");
            let first = stream
                .next()
                .await
                .expect("first stream item should exist")
                .expect("first stream item should be ok");
            assert_eq!(first, b"first".as_slice());
            let timeout_error = stream
                .next()
                .await
                .expect("second stream item should exist")
                .expect_err("second stream item should contain read timeout");
            assert_eq!(timeout_error.kind, HttpErrorKind::ReadTimeout);

            let _ = timeout(std::time::Duration::from_secs(3), server.finish())
                .await
                .expect("server finish timed out");
        });
    });
    assert!(logs.contains("Response body: <skipped: body not yet consumed>"));
}

#[test]
fn test_execute_logs_response_body_when_content_type_only_has_sse_prefix() {
    let logs = capture_trace_logs(|| {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime.block_on(async {
            let server = spawn_one_shot_server(ResponsePlan::Immediate {
                status: 200,
                headers: vec![(
                    "Content-Type".to_string(),
                    "text/event-streamish".to_string(),
                )],
                body: b"not an sse response".to_vec(),
            })
            .await;

            let mut options = HttpClientOptions::default();
            options.base_url = Some(server.base_url());
            options.logging.body_size_limit = 128;
            let mut policy_builder = RedactionPolicy::default().to_builder();
            policy_builder = policy_builder
                .http(|http| {
                    http.text_body(TextBodyPolicy::PassThrough);
                })
                .expect("test policy should be valid");
            options.log_redaction_policy = policy_builder
                .build()
                .expect("log redaction policy should be valid");
            let client = HttpClientBuilder::new()
                .options(options)
                .build()
                .expect("client should be created");

            let request = client.request(Method::GET, "/trace-not-sse-prefix").build();
            let mut response = timeout(std::time::Duration::from_secs(3), client.execute(request))
                .await
                .expect("execute timed out")
                .expect("request should succeed");
            assert_eq!(
                response.text().await.expect("body should remain readable"),
                "not an sse response"
            );

            let captured = timeout(std::time::Duration::from_secs(3), server.finish())
                .await
                .expect("server finish timed out");
            assert_eq!(captured.target, "/trace-not-sse-prefix");
        });
    });
    assert!(logs.contains("Response body: <skipped: body not yet consumed>"));
}

#[test]
fn test_log_stream_response_headers_disabled_emits_nothing() {
    let mut options = HttpLoggingOptions::default();
    options.enabled = false;
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    let logger = HttpLogger::new(&client_options);
    let response_meta = HttpResponseMeta::new(
        StatusCode::OK,
        HeaderMap::new(),
        Url::parse("https://example.com/stream-disabled").expect("URL should parse"),
        Method::GET,
    );

    let logs = capture_trace_logs(|| {
        logger.log_stream_response_headers(&response_meta);
    });
    assert!(logs.trim().is_empty());
}

#[test]
fn test_log_stream_response_headers_logs_non_utf8_header_values() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-bin",
        HeaderValue::from_bytes(&[0xFF, 0xFE]).expect("header bytes should be accepted"),
    );
    let mut client_options = HttpClientOptions::default();
    client_options.logging = HttpLoggingOptions::default();
    let logger = HttpLogger::new(&client_options);
    let response_meta = HttpResponseMeta::new(
        StatusCode::OK,
        headers,
        Url::parse("https://example.com/stream-non-utf8").expect("URL should parse"),
        Method::GET,
    );

    let logs = capture_trace_logs(|| {
        logger.log_stream_response_headers(&response_meta);
    });
    assert!(logs.contains("<-- 200 https://example.com/stream-non-utf8 (stream)"));
    assert!(logs.contains("x-bin: [<non-utf8>]"));
}

#[test]
fn test_log_stream_response_headers_masks_native_sensitive_value() {
    let mut headers = HeaderMap::new();
    let mut value = HeaderValue::from_static("native-response-header-secret");
    value.set_sensitive(true);
    headers.insert("x-diagnostic-value", value);
    let mut client_options = HttpClientOptions::default();
    client_options.logging = HttpLoggingOptions::default();
    let logger = HttpLogger::new(&client_options);
    let response_meta = HttpResponseMeta::new(
        StatusCode::OK,
        headers,
        Url::parse("https://example.com/stream-sensitive").expect("URL should parse"),
        Method::GET,
    );

    let logs = capture_trace_logs(|| {
        logger.log_stream_response_headers(&response_meta);
    });

    assert!(logs.contains("x-diagnostic-value: [<redacted>]"));
    assert!(!logs.contains("native-response-header-secret"));
}

#[test]
fn test_log_request_logs_json_form_multipart_ndjson_and_empty_bodies() {
    let mut client_options = HttpClientOptions::default();
    client_options.logging = HttpLoggingOptions::default();
    let logger = HttpLogger::new(&client_options);
    let client = HttpClientBuilder::new()
        .build()
        .expect("default options should create client");

    let json_request = client
        .request(Method::POST, "https://example.com/json-body")
        .json_body(&serde_json::json!({"n": 7}))
        .expect("json body should serialize")
        .build();
    let json_logs = capture_trace_logs(|| {
        logger.log_request(&json_request);
    });
    assert!(json_logs.contains("Request body: {\"n\":7}"));

    let form_request = client
        .request(Method::POST, "https://example.com/form-body")
        .form_body([("a", "1"), ("b", "2")])
        .build();
    let form_logs = capture_trace_logs(|| {
        logger.log_request(&form_request);
    });
    assert!(form_logs.contains("Request body: a=1&b=2"));

    let multipart_request = client
        .request(Method::POST, "https://example.com/multipart-body")
        .multipart_body(
            Bytes::from_static(b"--b\r\n\r\nraw-multipart-value\r\n--b--"),
            "b",
        )
        .expect("multipart body should be accepted")
        .build();
    let multipart_logs = capture_trace_logs(|| {
        logger.log_request(&multipart_request);
    });
    assert!(multipart_logs.contains("Request body:"));
    assert!(!multipart_logs.contains("--b"));
    assert!(!multipart_logs.contains("raw-multipart-value"));

    let ndjson_request = client
        .request(Method::POST, "https://example.com/ndjson-body")
        .ndjson_body(&[serde_json::json!({"id": 1}), serde_json::json!({"id": 2})])
        .expect("ndjson body should serialize")
        .build();
    let ndjson_logs = capture_trace_logs(|| {
        logger.log_request(&ndjson_request);
    });
    assert!(ndjson_logs.contains("Request body: {\"id\":1}"));
    assert!(ndjson_logs.contains("{\"id\":2}"));

    let empty_request = client
        .request(Method::GET, "https://example.com/empty-body")
        .build();
    let empty_logs = capture_trace_logs(|| {
        logger.log_request(&empty_request);
    });
    assert!(empty_logs.contains("Request body: <empty>"));
}

#[test]
fn test_log_response_empty_body_renders_empty_placeholder() {
    let mut client_options = HttpClientOptions::default();
    client_options.logging = HttpLoggingOptions::default();
    let logger = HttpLogger::new(&client_options);

    let logs = capture_trace_logs(|| {
        let mut response = HttpResponse::new(
            StatusCode::OK,
            HeaderMap::new(),
            Bytes::new(),
            Url::parse("https://example.com/empty-response").expect("URL should parse"),
            Method::GET,
        );
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime
            .block_on(async { logger.log_response(&mut response).await })
            .expect("response logging should succeed");
    });
    assert!(logs.contains("Response body: <empty>"));
}

#[test]
fn test_execute_logs_response_body_from_backend_when_trace_enabled() {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    let logs = capture_trace_logs(|| {
        runtime.block_on(async {
            let server = spawn_one_shot_server(ResponsePlan::Immediate {
                status: 200,
                headers: vec![],
                body: b"backend-log-body".to_vec(),
            })
            .await;

            let mut options = HttpClientOptions::default();
            options.base_url = Some(server.base_url());
            let client = HttpClientBuilder::new()
                .options(options)
                .build()
                .expect("client should be created");
            let request = client.request(Method::GET, "/logger-backend-path").build();
            let mut response = client
                .execute(request)
                .await
                .expect("request should succeed");
            let text = response
                .text()
                .await
                .expect("response text should be readable");
            assert_eq!(text, "backend-log-body");

            let captured = server.finish().await;
            assert_eq!(captured.target, "/logger-backend-path");
        });
    });
    assert!(logs.contains("Response body: <skipped: body not yet consumed>"));
}

#[test]
fn test_log_request_bytes_body_variant_is_logged() {
    let mut client_options = HttpClientOptions::default();
    client_options.logging = HttpLoggingOptions::default();
    let logger = HttpLogger::new(&client_options);
    let client = HttpClientBuilder::new()
        .build()
        .expect("default options should create client");

    let bytes_request = client
        .request(Method::POST, "https://example.com/bytes-body")
        .bytes_body(Bytes::from_static(b"raw-bytes"))
        .build();
    let logs = capture_trace_logs(|| {
        logger.log_request(&bytes_request);
    });
    assert!(logs.contains("Request body: <redacted: unsupported HTTP body>"));
}

#[test]
fn test_log_response_skips_body_when_backend_already_consumed() {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    let logs = capture_trace_logs(|| {
        runtime.block_on(async {
            let server = spawn_one_shot_server(ResponsePlan::Immediate {
                status: 200,
                headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
                body: b"already-consumed".to_vec(),
            })
            .await;

            let mut options = HttpClientOptions::default();
            options.base_url = Some(server.base_url());
            options.logging.enabled = false;
            let client = HttpClientBuilder::new()
                .options(options)
                .build()
                .expect("client should be created");
            let request = client.request(Method::GET, "/consumed-backend").build();
            let mut response = client
                .execute(request)
                .await
                .expect("request should succeed");

            let mut stream = response.stream().expect("stream should be available");
            let _ = stream
                .next()
                .await
                .expect("stream should yield one item")
                .expect("stream item should be valid");
            assert!(stream.next().await.is_none());
            drop(stream);

            let mut logger_options = HttpClientOptions::default();
            logger_options.logging = HttpLoggingOptions::default();
            let logger = HttpLogger::new(&logger_options);
            logger
                .log_response(&mut response)
                .await
                .expect("logging should succeed");

            let captured = server.finish().await;
            assert_eq!(captured.target, "/consumed-backend");
        });
    });
    assert!(logs.contains("Response body: <skipped: body not yet consumed>"));
}

#[test]
fn test_log_response_skips_body_for_sse_content_type() {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    let logs = capture_trace_logs(|| {
        runtime.block_on(async {
            let server = spawn_one_shot_server(ResponsePlan::Immediate {
                status: 200,
                headers: vec![(
                    "Content-Type".to_string(),
                    "text/event-stream; charset=utf-8".to_string(),
                )],
                body: b"data: hello\n\n".to_vec(),
            })
            .await;

            let mut options = HttpClientOptions::default();
            options.base_url = Some(server.base_url());
            options.logging.enabled = false;
            let client = HttpClientBuilder::new()
                .options(options)
                .build()
                .expect("client should be created");
            let request = client.request(Method::GET, "/sse-log-skip").build();
            let mut response = client
                .execute(request)
                .await
                .expect("request should succeed");

            let mut logger_options = HttpClientOptions::default();
            logger_options.logging = HttpLoggingOptions::default();
            let logger = HttpLogger::new(&logger_options);
            logger
                .log_response(&mut response)
                .await
                .expect("logging should succeed");

            let captured = server.finish().await;
            assert_eq!(captured.target, "/sse-log-skip");
        });
    });
    assert!(logs.contains("Response body: <skipped: body not yet consumed>"));
}

#[test]
fn test_cross_client_redaction_success_logs_follow_request_snapshot() {
    for (request_policy, expected_visible) in [
        (RedactionPolicy::strict(), false),
        (RedactionPolicy::default(), true),
    ] {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        let logs = capture_trace_logs(|| {
            runtime.block_on(async {
                let server = spawn_one_shot_server(ResponsePlan::Immediate {
                    status: 200,
                    headers: vec![],
                    body: b"ok".to_vec(),
                })
                .await;

                let mut request_options = HttpClientOptions::default();
                request_options.base_url = Some(server.base_url());
                request_options.log_redaction_policy = request_policy;
                let request_client = HttpClientBuilder::new()
                    .options(request_options)
                    .build()
                    .expect("request client should be created");

                let mut execute_options = HttpClientOptions::default();
                execute_options.base_url = Some(server.base_url());
                let execute_client = HttpClientBuilder::new()
                    .options(execute_options)
                    .build()
                    .expect("execute client should be created");
                let request = request_client
                    .request(Method::GET, "/cross-client-policy-sentinel")
                    .build();
                let _response = execute_client
                    .execute(request)
                    .await
                    .expect("request should succeed");

                let captured = server.finish().await;
                assert_eq!(captured.target, "/cross-client-policy-sentinel");
            });
        });

        assert!(logs.contains("--> GET"));
        assert!(logs.contains("<-- 200"));
        assert_eq!(
            logs.contains("cross-client-policy-sentinel"),
            expected_visible
        );
    }
}

#[test]
fn test_cross_client_redaction_rebuilt_client_keeps_old_and_new_snapshots() {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    let (old_server, new_server, rebuilt_client, old_request, new_request) =
        runtime.block_on(async {
            let old_server = spawn_one_shot_server(ResponsePlan::Immediate {
                status: 200,
                headers: vec![],
                body: b"old".to_vec(),
            })
            .await;
            let new_server = spawn_one_shot_server(ResponsePlan::Immediate {
                status: 200,
                headers: vec![],
                body: b"new".to_vec(),
            })
            .await;

            let mut old_options = HttpClientOptions::default();
            old_options.base_url = Some(old_server.base_url());
            old_options.log_redaction_policy = RedactionPolicy::strict();
            let old_client = HttpClientBuilder::new()
                .options(old_options)
                .build()
                .expect("old client should be created");
            let old_request = old_client
                .request(Method::GET, "/cross-client-policy-sentinel")
                .build();

            let mut new_options = HttpClientOptions::default();
            new_options.base_url = Some(new_server.base_url());
            new_options.log_redaction_policy = RedactionPolicy::default();
            let rebuilt_client = old_client
                .rebuild_with_options(new_options)
                .expect("client should be rebuilt");
            let new_request = rebuilt_client
                .request(Method::GET, "/cross-client-policy-sentinel")
                .build();

            (
                old_server,
                new_server,
                rebuilt_client,
                old_request,
                new_request,
            )
        });

    let old_logs = capture_trace_logs(|| {
        runtime.block_on(async {
            let _response = rebuilt_client
                .execute(old_request)
                .await
                .expect("old request should succeed");
        });
    });
    let new_logs = capture_trace_logs(|| {
        runtime.block_on(async {
            let _response = rebuilt_client
                .execute(new_request)
                .await
                .expect("new request should succeed");
        });
    });

    for logs in [&old_logs, &new_logs] {
        assert!(logs.contains("--> GET"));
        assert!(logs.contains("<-- 200"));
    }
    assert!(!old_logs.contains("cross-client-policy-sentinel"));
    assert!(new_logs.contains("cross-client-policy-sentinel"));

    let old_captured = runtime.block_on(old_server.finish());
    let new_captured = runtime.block_on(new_server.finish());
    assert_eq!(old_captured.target, "/cross-client-policy-sentinel");
    assert_eq!(new_captured.target, "/cross-client-policy-sentinel");
}

#[test]
fn test_cross_client_redaction_status_errors_follow_request_snapshot() {
    for (request_policy, expected_visible) in [
        (RedactionPolicy::strict(), false),
        (RedactionPolicy::default(), true),
    ] {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime.block_on(async {
            let server = spawn_one_shot_server(ResponsePlan::Immediate {
                status: 503,
                headers: vec![],
                body: b"retry later".to_vec(),
            })
            .await;

            let mut request_options = HttpClientOptions::default();
            request_options.base_url = Some(server.base_url());
            request_options.log_redaction_policy = request_policy;
            let request_client = HttpClientBuilder::new()
                .options(request_options)
                .build()
                .expect("request client should be created");

            let mut execute_options = HttpClientOptions::default();
            execute_options.base_url = Some(server.base_url());
            execute_options.retry.enabled = false;
            let execute_client = HttpClientBuilder::new()
                .options(execute_options)
                .build()
                .expect("execute client should be created");
            let request = request_client
                .request(Method::GET, "/cross-client-policy-sentinel")
                .build();
            let error = execute_client
                .execute(request)
                .await
                .expect_err("503 status should fail");

            assert_eq!(error.kind(), HttpErrorKind::Status);
            assert_eq!(error.status(), Some(StatusCode::SERVICE_UNAVAILABLE));
            assert_eq!(
                error.status_response().unwrap().body().as_ref(),
                b"retry later"
            );
            assert_eq!(
                format!("{error:?}").contains("cross-client-policy-sentinel"),
                expected_visible
            );
            let captured = server.finish().await;
            assert_eq!(captured.target, "/cross-client-policy-sentinel");
        });
    }
}

#[test]
fn test_cross_client_redaction_retry_exhaustion_preserves_error_context() {
    for (request_policy, expected_visible) in [
        (RedactionPolicy::strict(), false),
        (RedactionPolicy::default(), true),
    ] {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime.block_on(async {
            let server = spawn_multi_shot_server(
                (0..2)
                    .map(|_| ResponsePlan::Immediate {
                        status: 503,
                        headers: vec![],
                        body: b"retry later".to_vec(),
                    })
                    .collect(),
            )
            .await;

            let mut request_options = HttpClientOptions::default();
            request_options.base_url = Some(server.base_url());
            request_options.log_redaction_policy = request_policy;
            let request_client = HttpClientBuilder::new()
                .options(request_options)
                .build()
                .expect("request client should be created");

            let mut execute_options = HttpClientOptions::default();
            execute_options.base_url = Some(server.base_url());
            execute_options.retry.enabled = true;
            execute_options.retry.max_attempts = 2;
            execute_options.retry.backoff = BackoffPolicy::immediate();
            let execute_client = HttpClientBuilder::new()
                .options(execute_options)
                .build()
                .expect("execute client should be created");
            let request = request_client
                .request(Method::GET, "/cross-client-policy-sentinel")
                .build();
            let error = execute_client
                .execute(request)
                .await
                .expect_err("503 status should exhaust retry budget");

            assert_eq!(error.kind(), HttpErrorKind::Status);
            let diagnostics = error
                .retry_diagnostics()
                .expect("retry diagnostics should be retained");
            assert_eq!(diagnostics.attempts(), 2);
            let retry = std::error::Error::source(&error)
                .and_then(|source| source.downcast_ref::<RetryError<HttpError>>())
                .expect("RetryError<HttpError> source should be retained");
            let last_error = retry
                .last_error()
                .expect("last HTTP error should be retained");
            assert_eq!(last_error.status(), Some(StatusCode::SERVICE_UNAVAILABLE));
            assert_eq!(
                format!("{last_error:?}").contains("cross-client-policy-sentinel"),
                expected_visible
            );
            let captured = server.finish().await;
            assert_eq!(captured.len(), 2);
            assert!(captured
                .iter()
                .all(|request| request.target == "/cross-client-policy-sentinel"));
        });
    }
}

#[test]
fn test_cross_client_redaction_retry_wait_cancellation_preserves_context() {
    for (request_policy, expected_visible) in [
        (RedactionPolicy::strict(), false),
        (RedactionPolicy::default(), true),
    ] {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime.block_on(async {
            let mut request_options = HttpClientOptions::default();
            request_options
                .set_base_url("http://127.0.0.1:9")
                .expect("test base URL should parse");
            request_options.log_redaction_policy = request_policy;
            let request_client = HttpClientBuilder::new()
                .options(request_options)
                .build()
                .expect("request client should be created");

            let mut execute_options = HttpClientOptions::default();
            execute_options
                .set_base_url("http://127.0.0.1:9")
                .expect("test base URL should parse");
            execute_options.retry.enabled = true;
            execute_options.retry.max_attempts = 3;
            execute_options.retry.backoff = BackoffPolicy::fixed(std::time::Duration::from_secs(5));
            let mut execute_client = HttpClientBuilder::new()
                .options(execute_options)
                .build()
                .expect("execute client should be created");
            let attempt_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let observed_attempts = attempt_count.clone();
            execute_client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
                observed_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(HttpError::transport(
                    "deterministic retry failure at http://127.0.0.1:9/cross-client-policy-sentinel",
                ))
            }));

            let cancellation = HttpCancellationToken::new();
            let request = request_client
                .request(Method::GET, "/cross-client-policy-sentinel")
                .cancellation_token(cancellation.clone())
                .build();
            let future = execute_client.execute(request);
            tokio::pin!(future);
            let waker = std::task::Waker::noop();
            let mut context = std::task::Context::from_waker(waker);
            assert!(std::future::Future::poll(future.as_mut(), &mut context).is_pending());
            assert_eq!(attempt_count.load(std::sync::atomic::Ordering::SeqCst), 1);

            cancellation.cancel();
            let error = timeout(std::time::Duration::from_secs(3), future)
                .await
                .expect("cancelled retry wait timed out")
                .expect_err("cancellation during retry wait should fail");
            assert_eq!(error.kind(), HttpErrorKind::Cancelled);
            let retry = std::error::Error::source(&error)
                .and_then(|source| source.downcast_ref::<RetryError<HttpError>>())
                .expect("RetryError<HttpError> source should be retained");
            let last_error = retry.last_error().expect("last HTTP error should be retained");
            assert_eq!(last_error.kind(), HttpErrorKind::Transport);
            assert_eq!(
                format!("{error:?}").contains("cross-client-policy-sentinel"),
                expected_visible
            );
            assert_eq!(
                format!("{last_error:?}").contains("cross-client-policy-sentinel"),
                expected_visible
            );
            assert_eq!(
                last_error.to_string().contains("cross-client-policy-sentinel"),
                expected_visible
            );
        });
    }
}

#[test]
fn test_cross_client_redaction_sse_handshake_errors_follow_request_snapshot() {
    for (request_policy, expected_visible) in [
        (RedactionPolicy::strict(), false),
        (RedactionPolicy::default(), true),
    ] {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime.block_on(async {
            let server = spawn_one_shot_server(ResponsePlan::Immediate {
                status: 200,
                headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
                body: b"not an event stream".to_vec(),
            })
            .await;

            let mut request_options = HttpClientOptions::default();
            request_options.base_url = Some(server.base_url());
            request_options.log_redaction_policy = request_policy;
            let request_client = HttpClientBuilder::new()
                .options(request_options)
                .build()
                .expect("request client should be created");

            let mut execute_options = HttpClientOptions::default();
            execute_options.base_url = Some(server.base_url());
            let execute_client = HttpClientBuilder::new()
                .options(execute_options)
                .build()
                .expect("execute client should be created");
            let request = request_client
                .request(Method::GET, "/cross-client-policy-sentinel")
                .build();
            let mut events =
                execute_client.execute_sse_with_reconnect(request, SseReconnectOptions::default());
            let error = timeout(std::time::Duration::from_secs(3), events.next())
                .await
                .expect("SSE handshake timed out")
                .expect("SSE stream should return an error")
                .expect_err("non-SSE response should fail handshake");

            assert_eq!(error.kind(), HttpErrorKind::SseProtocol);
            assert_eq!(error.status(), Some(StatusCode::OK));
            assert_eq!(
                format!("{error:?}").contains("cross-client-policy-sentinel"),
                expected_visible
            );
            let captured = server.finish().await;
            assert_eq!(captured.target, "/cross-client-policy-sentinel");
        });
    }
}

#[test]
fn test_cross_client_redaction_sse_decode_errors_follow_request_snapshot() {
    for (request_policy, expected_visible) in [
        (RedactionPolicy::strict(), false),
        (RedactionPolicy::default(), true),
    ] {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime.block_on(async {
            let server = spawn_one_shot_server(ResponsePlan::Immediate {
                status: 200,
                headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
                body: b"data: not-json\n\n".to_vec(),
            })
            .await;

            let mut request_options = HttpClientOptions::default();
            request_options.base_url = Some(server.base_url());
            request_options.log_redaction_policy = request_policy;
            request_options.sse_json_mode = SseJsonMode::Strict;
            let request_client = HttpClientBuilder::new()
                .options(request_options)
                .build()
                .expect("request client should be created");

            let mut execute_options = HttpClientOptions::default();
            execute_options.base_url = Some(server.base_url());
            execute_options.sse_json_mode = SseJsonMode::Strict;
            let execute_client = HttpClientBuilder::new()
                .options(execute_options)
                .build()
                .expect("execute client should be created");
            let request = request_client
                .request(Method::GET, "/cross-client-policy-sentinel")
                .build();
            let response = execute_client
                .execute(request)
                .await
                .expect("SSE response handshake should succeed");
            let mut chunks = response.sse_chunks::<serde_json::Value>();
            let error = timeout(std::time::Duration::from_secs(3), chunks.next())
                .await
                .expect("SSE decoding timed out")
                .expect("SSE stream should return an error")
                .expect_err("invalid JSON event should fail decoding");

            assert_eq!(error.kind(), HttpErrorKind::SseDecode);
            assert_eq!(error.status(), Some(StatusCode::OK));
            assert!(
                std::error::Error::source(&error).is_some(),
                "decode source should be retained"
            );
            assert_eq!(
                format!("{error:?}").contains("cross-client-policy-sentinel"),
                expected_visible
            );
            let captured = server.finish().await;
            assert_eq!(captured.target, "/cross-client-policy-sentinel");
        });
    }
}

#[test]
fn test_cross_client_redaction_read_timeout_errors_follow_request_snapshot() {
    for (request_policy, expected_visible) in [
        (RedactionPolicy::strict(), false),
        (RedactionPolicy::default(), true),
    ] {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime.block_on(async {
            let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
                status: 200,
                headers: vec![],
                total_length: 64,
                prefix: b"partial-body".to_vec(),
                delay: std::time::Duration::from_millis(100),
            })
            .await;

            let mut request_options = HttpClientOptions::default();
            request_options.base_url = Some(server.base_url());
            request_options.log_redaction_policy = request_policy;
            request_options.timeouts.read_timeout = std::time::Duration::from_millis(20);
            let request_client = HttpClientBuilder::new()
                .options(request_options)
                .build()
                .expect("request client should be created");

            let mut execute_options = HttpClientOptions::default();
            execute_options.base_url = Some(server.base_url());
            execute_options.timeouts.read_timeout = std::time::Duration::from_millis(20);
            let execute_client = HttpClientBuilder::new()
                .options(execute_options)
                .build()
                .expect("execute client should be created");
            let request = request_client
                .request(Method::GET, "/cross-client-policy-sentinel")
                .build();
            let mut response = timeout(
                std::time::Duration::from_secs(3),
                execute_client.execute(request),
            )
            .await
            .expect("execute timed out")
            .expect("response headers should arrive");
            let error = timeout(std::time::Duration::from_secs(3), response.text())
                .await
                .expect("body read timed out")
                .expect_err("partial response should hit read timeout");

            assert_eq!(error.kind(), HttpErrorKind::ReadTimeout);
            assert_eq!(
                format!("{error:?}").contains("cross-client-policy-sentinel"),
                expected_visible
            );
            let _ = timeout(std::time::Duration::from_secs(3), server.finish())
                .await
                .expect("server finish timed out");
        });
    }
}
