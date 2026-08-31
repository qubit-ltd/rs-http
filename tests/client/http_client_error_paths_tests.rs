// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use http::Method;
use http::StatusCode;
use httpdate::fmt_http_date;
use qubit_http::HttpClientFactory;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use tokio::time::timeout;

use crate::common::ResponsePlan;
use crate::common::spawn_one_shot_server;

#[tokio::test]
async fn test_execute_maps_retry_after_to_retryable_http_error() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 429,
        headers: vec![("Retry-After".to_string(), "2".to_string())],
        body: b"too many requests".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
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

#[tokio::test]
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
    let client = HttpClientFactory::new()
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

#[tokio::test]
async fn test_execute_ignores_invalid_retry_after_for_service_unavailable() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 503,
        headers: vec![("Retry-After".to_string(), "not-a-valid-value".to_string())],
        body: b"service unavailable".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
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

#[tokio::test]
async fn test_execute_ignores_blank_retry_after_for_service_unavailable() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 503,
        headers: vec![("Retry-After".to_string(), "   ".to_string())],
        body: b"service unavailable".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientFactory::new()
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

#[tokio::test]
async fn test_execute_rejects_ipv6_url_when_ipv4_only() {
    let mut options = HttpClientOptions::default();
    options.ipv4_only = true;
    let client = HttpClientFactory::new()
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

#[tokio::test]
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
    let client = HttpClientFactory::new()
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
