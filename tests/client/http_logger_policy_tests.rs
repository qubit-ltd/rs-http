// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use http::StatusCode;
use http::header::AUTHORIZATION;
use http::header::CONTENT_TYPE;
use http::header::SET_COOKIE;
use qubit_http::HttpClientFactory;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use qubit_http::HttpLogger;
use qubit_http::HttpLoggingOptions;
use qubit_http::HttpRequest;
use qubit_http::HttpRequestBody;
use qubit_http::HttpRequestBodyByteStream;
use qubit_http::HttpResponse;
use qubit_http::HttpResponseMeta;
use qubit_redact::RedactionPolicy;
use qubit_redact::http::TextBodyPolicy;
use tokio::time::timeout;
use url::Url;

use crate::common::ResponseChunk;
use crate::common::ResponsePlan;
use crate::common::capture_trace_logs;
use crate::common::spawn_one_shot_server;

fn logging_request(
    method: Method,
    path: &str,
    headers: HeaderMap,
    body: HttpRequestBody,
) -> HttpRequest {
    let client = HttpClientFactory::new()
        .create_default()
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
            Url::parse("https://example.com/data").unwrap(),
            Method::GET,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
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
        let runtime = tokio::runtime::Builder::new_current_thread()
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
            Url::parse("https://example.com/bin").unwrap(),
            Method::GET,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime
            .block_on(async { logger.log_response(&mut response).await })
            .expect("response logging should succeed");
    });
    assert!(logs.contains("Response body: <binary 4 bytes><truncated>"));
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
            Url::parse("https://example.com/stream").unwrap(),
            Method::GET,
        );
        logger.log_stream_response_headers(&response_meta);
    });
    assert!(logs.contains("<-- 200 https://example.com/stream (stream)"));
    assert!(!logs.contains("text/event-stream"));
}

#[test]
fn test_log_request_includes_builder_query_params() {
    let client = HttpClientFactory::new()
        .create_default()
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

    assert!(logs.contains(
        "--> GET https://example.com/api?existing=1&added=two+words"
    ));
}

#[test]
fn test_log_request_text_body() {
    let options = HttpLoggingOptions::default();
    let headers = HeaderMap::new();
    let mut client_options = HttpClientOptions::default();
    client_options.logging = options;
    client_options.log_redaction_policy = RedactionPolicy::default()
        .to_builder()
        .text_body_policy(TextBodyPolicy::PassThrough)
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

    assert!(
        logs.contains("Request body: <redacted: invalid content type body>")
    );
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
            Url::parse("https://example.com/text")
                .expect("response URL should parse"),
            Method::GET,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
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
        HttpRequestBody::Stream(vec![
            Bytes::from_static(b"a"),
            Bytes::from_static(b"b"),
        ]),
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
    let client = HttpClientFactory::new()
        .create_default()
        .expect("default options should create client");

    let request = client
        .request(Method::POST, "https://example.com/streaming-upload")
        .streaming_body(|| {
            Box::pin(async move {
                Box::pin(stream::iter([Ok(Bytes::from_static(
                    b"secret-stream",
                ))])) as HttpRequestBodyByteStream
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
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime.block_on(async {
            let server =
                spawn_one_shot_server(ResponsePlan::PartialThenDelay {
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
            let client = HttpClientFactory::new()
                .create(options)
                .expect("client should be created");

            let request = client
                .request(Method::GET, "/trace-body-read-error")
                .build();
            let error = timeout(
                std::time::Duration::from_secs(3),
                client.execute(request),
            )
            .await
            .expect("execute timed out")
            .expect_err("response logging should surface body read failure");
            assert_eq!(error.kind, HttpErrorKind::Transport);
            assert_eq!(error.method, Some(Method::GET));
            assert!(
                error
                    .url
                    .as_ref()
                    .is_some_and(|url| url.path() == "/trace-body-read-error")
            );

            let captured =
                timeout(std::time::Duration::from_secs(3), server.finish())
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
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime.block_on(async {
            let server = spawn_one_shot_server(ResponsePlan::Chunked {
                status: 200,
                headers: vec![(
                    "Content-Type".to_string(),
                    "text/plain".to_string(),
                )],
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
            options.timeouts.read_timeout =
                std::time::Duration::from_millis(80);
            let client = HttpClientFactory::new()
                .create(options)
                .expect("client should be created");

            let request =
                client.request(Method::GET, "/trace-streaming").build();
            let mut response = timeout(
                std::time::Duration::from_secs(3),
                client.execute(request),
            )
            .await
            .expect("execute timed out")
            .expect("request should start");
            let mut stream =
                response.stream().expect("stream body should be available");
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
    assert!(
        logs.contains(
            "Response body: <skipped: streaming or unknown-size body>"
        )
    );
}

#[test]
fn test_execute_logs_response_body_when_content_type_only_has_sse_prefix() {
    let logs = capture_trace_logs(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
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
            options.log_redaction_policy = RedactionPolicy::default()
                .to_builder()
                .text_body_policy(TextBodyPolicy::PassThrough)
                .build()
                .expect("log redaction policy should be valid");
            let client = HttpClientFactory::new()
                .create(options)
                .expect("client should be created");

            let request =
                client.request(Method::GET, "/trace-not-sse-prefix").build();
            let mut response = timeout(
                std::time::Duration::from_secs(3),
                client.execute(request),
            )
            .await
            .expect("execute timed out")
            .expect("request should succeed");
            assert_eq!(
                response.text().await.expect("body should remain readable"),
                "not an sse response"
            );

            let captured =
                timeout(std::time::Duration::from_secs(3), server.finish())
                    .await
                    .expect("server finish timed out");
            assert_eq!(captured.target, "/trace-not-sse-prefix");
        });
    });
    assert!(logs.contains("Response body: not an sse response"));
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
        Url::parse("https://example.com/stream-disabled")
            .expect("URL should parse"),
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
        HeaderValue::from_bytes(&[0xFF, 0xFE])
            .expect("header bytes should be accepted"),
    );
    let mut client_options = HttpClientOptions::default();
    client_options.logging = HttpLoggingOptions::default();
    let logger = HttpLogger::new(&client_options);
    let response_meta = HttpResponseMeta::new(
        StatusCode::OK,
        headers,
        Url::parse("https://example.com/stream-non-utf8")
            .expect("URL should parse"),
        Method::GET,
    );

    let logs = capture_trace_logs(|| {
        logger.log_stream_response_headers(&response_meta);
    });
    assert!(
        logs.contains("<-- 200 https://example.com/stream-non-utf8 (stream)")
    );
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
        Url::parse("https://example.com/stream-sensitive")
            .expect("URL should parse"),
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
    let client = HttpClientFactory::new()
        .create_default()
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
        .ndjson_body(&[
            serde_json::json!({"id": 1}),
            serde_json::json!({"id": 2}),
        ])
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
            Url::parse("https://example.com/empty-response")
                .expect("URL should parse"),
            Method::GET,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
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
    let runtime = tokio::runtime::Builder::new_current_thread()
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
            let client = HttpClientFactory::new()
                .create(options)
                .expect("client should be created");
            let request =
                client.request(Method::GET, "/logger-backend-path").build();
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
    assert!(logs.contains("Response body: <redacted: unsupported HTTP body>"));
}

#[test]
fn test_log_request_bytes_body_variant_is_logged() {
    let mut client_options = HttpClientOptions::default();
    client_options.logging = HttpLoggingOptions::default();
    let logger = HttpLogger::new(&client_options);
    let client = HttpClientFactory::new()
        .create_default()
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
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    let logs = capture_trace_logs(|| {
        runtime.block_on(async {
            let server = spawn_one_shot_server(ResponsePlan::Immediate {
                status: 200,
                headers: vec![(
                    "Content-Type".to_string(),
                    "text/plain".to_string(),
                )],
                body: b"already-consumed".to_vec(),
            })
            .await;

            let mut options = HttpClientOptions::default();
            options.base_url = Some(server.base_url());
            options.logging.enabled = false;
            let client = HttpClientFactory::new()
                .create(options)
                .expect("client should be created");
            let request =
                client.request(Method::GET, "/consumed-backend").build();
            let mut response = client
                .execute(request)
                .await
                .expect("request should succeed");

            let mut stream =
                response.stream().expect("stream should be available");
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
    assert!(
        logs.contains(
            "Response body: <skipped: streaming or unknown-size body>"
        )
    );
}

#[test]
fn test_log_response_skips_body_for_sse_content_type() {
    let runtime = tokio::runtime::Builder::new_current_thread()
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
            let client = HttpClientFactory::new()
                .create(options)
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
    assert!(
        logs.contains(
            "Response body: <skipped: streaming or unknown-size body>"
        )
    );
}
