// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use bytes::Bytes;
use futures_util::StreamExt;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use http::StatusCode;
use http::header::CONTENT_LENGTH;
use http::header::RETRY_AFTER;
use qubit_http::HttpCancellationToken;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use qubit_http::HttpResponse;
use qubit_http::constants::DEFAULT_RESPONSE_BODY_SIZE_LIMIT_BYTES;
use qubit_json::decode::DiagnosticPolicy;
use qubit_json::decode::JsonDecodeError;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::test as tokio_test;
use tokio::time::timeout;
use url::Url;

use crate::common::ResponsePlan;
use crate::common::SensitiveChoice;
use crate::common::spawn_one_shot_server;

fn sse_validation_response(content_type: Option<HeaderValue>) -> HttpResponse {
    let mut headers = HeaderMap::new();
    if let Some(content_type) = content_type {
        headers.insert(http::header::CONTENT_TYPE, content_type);
    }
    HttpResponse::new(
        StatusCode::BAD_GATEWAY,
        headers,
        Bytes::from_static(b"event body"),
        Url::parse("https://example.com/events?access_token=sse-query-secret").unwrap(),
        Method::POST,
    )
}

#[test]
fn test_validate_sse_content_type_accepts_sse_media_types() {
    for content_type in ["text/event-stream", "Text/Event-Stream; charset=utf-8"] {
        let response = sse_validation_response(Some(HeaderValue::from_str(content_type).unwrap()));
        assert!(response.validate_sse_content_type().is_ok(), "{content_type}");
    }
}

#[test]
fn test_validate_sse_content_type_rejects_invalid_headers_with_response_context() {
    let invalid_values = [
        None,
        Some(HeaderValue::from_static("application/json")),
        Some(HeaderValue::from_static("text/event-streaming")),
        Some(HeaderValue::from_bytes(b"\xff").unwrap()),
    ];
    for content_type in invalid_values {
        let response = sse_validation_response(content_type);
        let error = response.validate_sse_content_type().unwrap_err();
        assert_eq!(error.kind, HttpErrorKind::SseProtocol);
        assert_eq!(error.status, Some(StatusCode::BAD_GATEWAY));
        assert_eq!(error.method, Some(Method::POST));
        assert_eq!(
            error.url,
            Some(Url::parse("https://example.com/events?access_token=sse-query-secret").unwrap())
        );
        assert!(!format!("{error}").contains("sse-query-secret"));
        assert!(!format!("{error:?}").contains("sse-query-secret"));
    }
}

#[tokio_test]
async fn test_validate_sse_content_type_does_not_consume_body() {
    let mut valid = sse_validation_response(Some(HeaderValue::from_static("text/event-stream")));
    valid.validate_sse_content_type().unwrap();
    assert_eq!(valid.bytes().await.unwrap(), Bytes::from_static(b"event body"));

    let mut invalid = sse_validation_response(Some(HeaderValue::from_static("application/json")));
    assert_eq!(
        invalid.validate_sse_content_type().unwrap_err().kind,
        HttpErrorKind::SseProtocol
    );
    assert_eq!(invalid.bytes().await.unwrap(), Bytes::from_static(b"event body"));
}

async fn cached_response(body: &'static [u8], content_type: &'static str) -> (HttpResponse, HttpCancellationToken) {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![("Content-Type".to_string(), content_type.to_string())],
        body: body.to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.proxy.enabled = false;
    options.use_env_proxy = false;
    options.retry.enabled = false;
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");
    let token = HttpCancellationToken::new();
    let request = client
        .request(Method::GET, "/cached-after-cancel")
        .cancellation_token(token.clone())
        .build();
    let mut response = timeout(std::time::Duration::from_secs(3), client.execute(request))
        .await
        .expect("request timed out")
        .expect("request should succeed");
    assert_eq!(
        response.bytes().await.expect("body should cache"),
        Bytes::from_static(body)
    );
    timeout(std::time::Duration::from_secs(3), server.finish())
        .await
        .expect("test server did not finish");
    (response, token)
}

#[tokio_test]
async fn test_cached_body_remains_readable_after_cancellation() {
    let (mut response, token) = cached_response(b"cached", "text/plain").await;
    token.cancel();
    assert_eq!(response.bytes().await.unwrap(), Bytes::from_static(b"cached"));

    let (mut response, token) = cached_response(b"cached", "text/plain").await;
    token.cancel();
    assert_eq!(response.text().await.unwrap(), "cached");

    let (mut response, token) = cached_response(b"{\"ok\":true}", "application/json").await;
    token.cancel();
    assert_eq!(response.json::<serde_json::Value>().await.unwrap()["ok"], true);

    let (mut response, token) = cached_response(b"cached", "text/plain").await;
    token.cancel();
    let mut stream = response.stream().expect("cached stream should remain readable");
    assert_eq!(stream.next().await.unwrap().unwrap(), Bytes::from_static(b"cached"));
    assert!(stream.next().await.is_none());

    let (response, token) = cached_response(b"data: value\n\n", "text/event-stream").await;
    token.cancel();
    let mut events = response.sse_messages();
    assert_eq!(events.next().await.unwrap().unwrap().data, "value");
    assert!(events.next().await.is_none());
}

#[tokio_test]
async fn test_304_content_length_is_not_body_length() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 1024];
        let _ = timeout(std::time::Duration::from_secs(2), stream.read(&mut request))
            .await
            .unwrap()
            .unwrap();
        stream
            .write_all(b"HTTP/1.1 304 Not Modified\r\nContent-Length: 100000\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    });
    let mut options = HttpClientOptions::default();
    options.base_url = Some(Url::parse(&format!("http://{address}")).unwrap());
    options.proxy.enabled = false;
    options.use_env_proxy = false;
    options.retry.enabled = false;
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client.request(Method::GET, "/not-modified").build();
    let error = timeout(std::time::Duration::from_secs(3), client.execute(request))
        .await
        .unwrap()
        .unwrap_err();
    let response = error
        .status_response()
        .unwrap_or_else(|| panic!("status error should preserve response: {error:?}"));
    assert_eq!(error.status(), Some(StatusCode::NOT_MODIFIED));
    assert!(response.body().is_empty());
    assert!(!response.is_truncated());
    assert_eq!(response.content_length(), Some(0));
    assert_eq!(response.headers().get(CONTENT_LENGTH).unwrap(), "100000");
    server.await.unwrap();
}

#[tokio_test]
async fn test_cached_stream_preserves_buffered_body() {
    let body = Bytes::from_static(b"cached");
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        body.clone(),
        Url::parse("https://example.com/cached").unwrap(),
        Method::GET,
    );

    assert_eq!(response.bytes().await.unwrap(), body);
    for _ in 0..2 {
        let mut stream = response.stream().unwrap();
        let chunk = stream.next().await.unwrap().unwrap();
        assert_eq!(chunk, body);
        assert!(stream.next().await.is_none());
    }
    assert_eq!(response.bytes().await.unwrap(), body);
}

#[tokio_test]
async fn test_cached_stream_can_be_dropped_and_reused_after_text_and_json() {
    let body = Bytes::from_static(b"{\"ok\":true}");
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        body.clone(),
        Url::parse("https://example.com/cached-json").unwrap(),
        Method::GET,
    );
    let stream = response.stream().unwrap();
    drop(stream);
    assert_eq!(response.text().await.unwrap(), "{\"ok\":true}");
    assert_eq!(response.json::<serde_json::Value>().await.unwrap()["ok"], true);
    let mut stream = response.stream().unwrap();
    assert_eq!(stream.next().await.unwrap().unwrap(), body);
    assert!(stream.next().await.is_none());
    assert_eq!(response.bytes().await.unwrap(), body);
}

#[tokio_test]
async fn test_http_response_buffered_body_limit_error_is_preserved_across_reads() {
    let url = Url::parse("https://example.com/oversized").expect("test URL must parse");
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from(vec![0; DEFAULT_RESPONSE_BODY_SIZE_LIMIT_BYTES + 1]),
        url.clone(),
        Method::GET,
    );

    let first = response
        .bytes()
        .await
        .expect_err("oversized buffered body must be rejected");
    assert_eq!(first.kind, HttpErrorKind::ResponseBodyTooLarge);
    assert_eq!(first.status, Some(StatusCode::OK));
    assert_eq!(first.url, Some(url.clone()));

    let second = response
        .bytes()
        .await
        .expect_err("later reads must retain the limit failure");
    assert_eq!(second.kind, HttpErrorKind::ResponseBodyTooLarge);
    assert_eq!(second.status, Some(StatusCode::OK));
    assert_eq!(second.url, Some(url));

    let stream_error = match response.stream() {
        Ok(_) => panic!("streaming must retain the earlier body limit failure"),
        Err(error) => error,
    };
    assert_eq!(stream_error.kind, HttpErrorKind::ResponseBodyTooLarge);
}

#[tokio_test]
async fn test_http_response_text_decode_error_contains_status_and_url() {
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(&[0xFF, 0xFE]),
        Url::parse("https://example.com/bin").unwrap(),
        Method::POST,
    );
    let error = response.text().await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert_eq!(error.method, Some(Method::POST));
    assert_eq!(error.status, Some(StatusCode::OK));
    assert_eq!(error.url, Some(Url::parse("https://example.com/bin").unwrap()));
}

#[test]
fn test_http_response_debug_masks_sensitive_values() {
    let mut headers = HeaderMap::new();
    headers.insert("set-cookie", HeaderValue::from_static("session=debug-cookie-secret"));
    let mut url = Url::parse("https://debug-user:debug-url-secret@example.com/data?access_token=debug-query-secret")
        .expect("URL should parse");
    url.set_fragment(Some("debug-fragment-secret"));
    let response = HttpResponse::new(
        StatusCode::OK,
        headers,
        Bytes::from_static(b"debug-response-body-secret"),
        url,
        Method::GET,
    );

    let debug = format!("{response:?}");

    assert!(!debug.contains("debug-user"));
    assert!(!debug.contains("debug-url-secret"));
    assert!(!debug.contains("debug-query-secret"));
    assert!(!debug.contains("debug-fragment-secret"));
    assert!(!debug.contains("debug-cookie-secret"));
    assert!(!debug.contains("debug-response-body-secret"));
    assert!(debug.contains("****"));
}

#[tokio_test]
async fn test_http_response_json_decode_error_contains_status_and_url() {
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"not-json"),
        Url::parse("https://example.com/json").unwrap(),
        Method::POST,
    );
    let error = response.json::<serde_json::Value>().await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert_eq!(error.method, Some(Method::POST));
    assert_eq!(error.status, Some(StatusCode::OK));
    assert_eq!(error.url, Some(Url::parse("https://example.com/json").unwrap()));
}

#[test]
fn test_http_response_is_success_reports_status_class() {
    let response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"ok"),
        Url::parse("https://example.com/ok").unwrap(),
        Method::GET,
    );

    assert!(response.is_success());
    assert!(
        !HttpResponse::new(
            StatusCode::BAD_REQUEST,
            HeaderMap::new(),
            Bytes::new(),
            Url::parse("https://example.com/bad").unwrap(),
            Method::GET,
        )
        .is_success()
    );
}

#[test]
fn test_http_response_meta_accessor_returns_shared_metadata() {
    let response = HttpResponse::new(
        StatusCode::ACCEPTED,
        HeaderMap::new(),
        Bytes::from_static(b"queued"),
        Url::parse("https://example.com/jobs/1").unwrap(),
        Method::POST,
    );

    let meta = response.meta();
    assert_eq!(meta.status(), StatusCode::ACCEPTED);
    assert_eq!(meta.url(), &Url::parse("https://example.com/jobs/1").unwrap());
    assert_eq!(meta.method(), &Method::POST);
}

#[test]
fn test_http_response_retry_after_hint_handles_applicable_status_and_past_date() {
    let mut headers = HeaderMap::new();
    headers.insert(RETRY_AFTER, HeaderValue::from_static("Wed, 21 Oct 2015 07:28:00 GMT"));
    let response = HttpResponse::new(
        StatusCode::SERVICE_UNAVAILABLE,
        headers.clone(),
        Bytes::new(),
        Url::parse("https://example.com/retry-after").unwrap(),
        Method::GET,
    );
    assert_eq!(response.retry_after_hint(), Some(std::time::Duration::ZERO));

    let success = HttpResponse::new(
        StatusCode::OK,
        headers,
        Bytes::new(),
        Url::parse("https://example.com/no-retry-after").unwrap(),
        Method::GET,
    );
    assert_eq!(success.retry_after_hint(), None);
}

#[tokio_test]
async fn test_http_response_text_success_returns_body() {
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"hello"),
        Url::parse("https://example.com/utf8").unwrap(),
        Method::GET,
    );

    let text = response.text().await.expect("valid utf8 should decode");
    assert_eq!(text, "hello");
}

#[tokio_test]
async fn test_http_response_json_success_decodes_value() {
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"{\"n\":42}"),
        Url::parse("https://example.com/json-ok").unwrap(),
        Method::GET,
    );

    let value = response
        .json::<serde_json::Value>()
        .await
        .expect("json payload should decode");
    assert_eq!(value["n"], 42);
}

#[tokio_test]
async fn test_http_response_json_rejects_markdown_fence() {
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"```json\n{\"n\":42}\n```"),
        Url::parse("https://example.com/json-fence").unwrap(),
        Method::GET,
    );

    let error = response
        .json::<serde_json::Value>()
        .await
        .expect_err("strict HTTP response JSON must reject Markdown fences");
    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert_eq!(error.status, Some(StatusCode::OK));
}

#[tokio_test]
async fn test_http_response_json_redacts_deserializer_value() {
    const SECRET: &str = "HTTP_TOP_SECRET";
    let mut response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from(format!("\"{SECRET}\"")),
        Url::parse("https://example.com/secure-json").expect("test URL must parse"),
        Method::GET,
    );
    let error = response
        .json::<SensitiveChoice>()
        .await
        .expect_err("unknown enum variant must fail");
    assert!(!error.message.contains(SECRET));
    assert_eq!(error.status, Some(StatusCode::OK));
    assert_eq!(
        error.url,
        Some(Url::parse("https://example.com/secure-json").expect("test URL must parse"))
    );
    let source =
        std::error::Error::source(&error).expect("HTTP JSON decode errors must retain the redacted decoder source");
    let decode_error = source
        .downcast_ref::<JsonDecodeError>()
        .expect("HTTP JSON decode source must be JsonDecodeError");
    assert_eq!(decode_error.diagnostic_policy(), DiagnosticPolicy::Redacted,);
    assert!(!decode_error.to_string().contains(SECRET));
}
