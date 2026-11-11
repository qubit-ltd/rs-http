// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use http::Method;
use http::StatusCode;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;

use crate::common::spawn_one_shot_server;
use crate::common::ResponsePlan;

#[tokio::test]
async fn test_head_contract_success_ignores_content_length_for_body_limit() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![("Content-Length".to_string(), "100".to_string())],
        body: Vec::new(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.response_body_size_limit = 1;
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let mut response = client
        .execute(client.request(Method::HEAD, "/head-200").build())
        .await
        .unwrap();

    assert_eq!(response.headers().get("content-length").unwrap(), "100");
    assert!(response.bytes().await.unwrap().is_empty());
    assert!(response.bytes().await.unwrap().is_empty());
    assert!(response.text().await.unwrap().is_empty());
    assert!(response.text().await.unwrap().is_empty());
    assert_eq!(server.finish().await.method, "HEAD");
}

#[tokio::test]
async fn test_head_contract_status_error_has_empty_complete_body_capture() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 404,
        headers: vec![("Content-Length".to_string(), "100".to_string())],
        body: Vec::new(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.response_body_size_limit = 1;
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let error = client
        .execute(client.request(Method::HEAD, "/head-404").build())
        .await
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::NOT_FOUND));
    let capture = error.status_response().unwrap();
    assert_eq!(capture.headers().get("content-length").unwrap(), "100");
    assert!(capture.body().is_empty());
    assert!(!capture.is_truncated());
    assert_eq!(server.finish().await.method, "HEAD");
}

#[tokio::test]
async fn test_head_contract_get_body_limit_reads_declared_response_body() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: Vec::new(),
        body: vec![b'x'; 100],
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.response_body_size_limit = 1;
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client
        .request(Method::GET, "/get-200")
        .disable_retry()
        .build();
    let mut response = client.execute(request).await.unwrap();

    let error = response.bytes().await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::ResponseBodyTooLarge);
    assert_eq!(server.finish().await.method, "GET");
}
