// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Representative contracts for downstream HTTP and SSE clients.

use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use http::HeaderMap;
use http::Method;
use http::StatusCode;
use qubit_http::HttpBodyCaptureEnd;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use qubit_http::HttpResponse;
use qubit_http::sse::DoneMarkerPolicy;
use qubit_http::sse::SseChunk;
use qubit_http::sse::SseCompletionPolicy;
use qubit_http::sse::SseJsonMode;
use serde::Deserialize;
use tokio::test as tokio_test;
use tokio::time::timeout;
use url::Url;

use crate::common::ResponsePlan;
use crate::common::spawn_multi_shot_server;
use crate::common::spawn_one_shot_server;

/// Example SDK policy kept local to these contract tests: only the provider's
/// rate-limit code is retryable; quota and unknown codes are terminal.
#[derive(Debug, PartialEq, Eq)]
enum ProviderDisposition {
    Retry,
    Stop,
}

fn classify_provider_error(raw: &qubit_http::HttpStatusResponse) -> Option<ProviderDisposition> {
    if raw.is_truncated() {
        return None;
    }
    let body: serde_json::Value = serde_json::from_slice(raw.body()).ok()?;
    match body.pointer("/error/code")?.as_str()? {
        "rate_limit_exceeded" => Some(ProviderDisposition::Retry),
        "insufficient_quota" => Some(ProviderDisposition::Stop),
        _ => Some(ProviderDisposition::Stop),
    }
}

#[tokio_test]
async fn test_downstream_contract_complete_429_preserves_metadata_and_provider_classification() {
    let body = br#"{"error":{"code":"rate_limit_exceeded","message":"slow down"}}"#.to_vec();
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 429,
        headers: vec![
            ("Content-Type".into(), "application/json".into()),
            ("x-request-id".into(), "req-contract-42".into()),
            ("Retry-After".into(), "3".into()),
        ],
        body: body.clone(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client.request(Method::GET, "/complete-429").disable_retry().build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .unwrap()
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::TOO_MANY_REQUESTS));
    assert_eq!(error.retry_after, Some(Duration::from_secs(3)));
    let capture = error.status_response().expect("raw status response should be retained");
    assert!(!capture.is_truncated());
    assert_eq!(capture.capture_end(), HttpBodyCaptureEnd::Complete);
    assert_eq!(capture.body().as_ref(), body.as_slice());
    assert_eq!(capture.headers()["x-request-id"], "req-contract-42");
    assert_eq!(classify_provider_error(capture), Some(ProviderDisposition::Retry));
    server.finish().await;
}

#[tokio_test]
async fn test_downstream_contract_quota_and_unknown_provider_codes_stop() {
    for code in ["insufficient_quota", "future_provider_code"] {
        let body = format!(r#"{{"error":{{"code":"{code}"}}}}"#).into_bytes();
        let server = spawn_one_shot_server(ResponsePlan::Immediate {
            status: 429,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body,
        })
        .await;
        let mut options = HttpClientOptions::default();
        options.base_url = Some(server.base_url());
        let client = HttpClientBuilder::new().options(options).build().unwrap();
        let request = client
            .request(Method::GET, "/provider-classification")
            .disable_retry()
            .build();
        let error = timeout(Duration::from_secs(3), client.execute(request))
            .await
            .unwrap()
            .unwrap_err();
        let capture = error.status_response().unwrap();
        assert_eq!(classify_provider_error(capture), Some(ProviderDisposition::Stop));
        server.finish().await;
    }
}

#[tokio_test]
async fn test_downstream_contract_incomplete_status_body_is_never_provider_classified() {
    let body = br#"{"error":{"code":"rate_limit_exceeded"}}"#;
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 429,
        headers: vec![("Content-Type".into(), "application/json".into())],
        total_length: body.len() + 50,
        prefix: body.to_vec(),
        delay: Duration::from_millis(10),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(1);
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client
        .request(Method::GET, "/incomplete-status")
        .disable_retry()
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .unwrap()
        .unwrap_err();
    let capture = error.status_response().expect("partial status body should be retained");
    assert!(capture.is_truncated());
    assert_eq!(capture.capture_end(), HttpBodyCaptureEnd::ReadFailure);
    assert_eq!(classify_provider_error(capture), None);
    server.finish().await;
}

#[tokio_test]
async fn test_downstream_contract_outer_two_attempts_disable_internal_http_retries() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 429,
            headers: vec![],
            body: b"first".to_vec(),
        },
        ResponsePlan::Immediate {
            status: 429,
            headers: vec![],
            body: b"second".to_vec(),
        },
    ])
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 4;
    options.retry.backoff = qubit_retry::BackoffPolicy::immediate();
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    for _ in 0..2 {
        let request = client.request(Method::GET, "/outer-attempt").disable_retry().build();
        let error = timeout(Duration::from_secs(3), client.execute(request))
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(error.status, Some(StatusCode::TOO_MANY_REQUESTS));
    }
    let requests = timeout(Duration::from_secs(3), server.finish()).await.unwrap();
    assert_eq!(
        requests.len(),
        2,
        "each outer attempt must issue exactly one HTTP request"
    );
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct SsePayload {
    value: i32,
}

fn sse_response(body: &'static str) -> HttpResponse {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "text/event-stream".parse().unwrap());
    HttpResponse::new(
        StatusCode::OK,
        headers,
        Bytes::from(body),
        Url::parse("https://example.test/events").unwrap(),
        Method::GET,
    )
}

#[tokio_test]
async fn test_downstream_contract_strict_sse_requires_valid_json_and_done_marker() {
    let complete = sse_response("data: {\"value\":7}\n\ndata: [DONE]\n\n");
    complete.validate_sse_content_type().unwrap();
    let chunks: Vec<_> = complete
        .sse_json_mode(SseJsonMode::Strict)
        .sse_done_marker_policy(DoneMarkerPolicy::DefaultDone)
        .sse_completion_policy(SseCompletionPolicy::RequireDoneMarker)
        .sse_chunks::<SsePayload>()
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(Result::unwrap)
        .collect();
    assert_eq!(chunks, vec![SseChunk::Data(SsePayload { value: 7 }), SseChunk::Done]);

    let missing_done = sse_response("data: {\"value\":7}\n\n");
    missing_done.validate_sse_content_type().unwrap();
    let mut chunks = missing_done
        .sse_json_mode(SseJsonMode::Strict)
        .sse_completion_policy(SseCompletionPolicy::RequireDoneMarker)
        .sse_chunks::<SsePayload>();
    assert!(matches!(chunks.next().await.unwrap().unwrap(), SseChunk::Data(_)));
    assert_eq!(
        chunks.next().await.unwrap().unwrap_err().kind,
        HttpErrorKind::SseProtocol
    );

    let invalid_json = sse_response("data: not-json\n\n");
    invalid_json.validate_sse_content_type().unwrap();
    let error = invalid_json
        .sse_json_mode(SseJsonMode::Strict)
        .sse_chunks::<SsePayload>()
        .next()
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::SseDecode);
}
