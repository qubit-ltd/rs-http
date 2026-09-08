// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public behavior coverage for private attempt cancellation routing.

use std::error::Error as _;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use http::HeaderMap;
use http::Method;
use qubit_http::AsyncHttpHeaderInjector;
use qubit_http::HttpCancellationToken;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpError;
use qubit_http::HttpRequest;
use qubit_http::HttpRequestInterceptor;
use qubit_http::HttpResult;
use qubit_retry::BackoffPolicy;
use qubit_retry::RetryError;

use crate::common::spawn_multi_shot_server;

/// Returns the structured retry terminal chained by an HTTP cancellation.
fn retry_failure(error: &HttpError) -> &RetryError<HttpError> {
    error
        .source()
        .and_then(|source| source.downcast_ref::<RetryError<HttpError>>())
        .expect("retry cancellation should retain RetryError")
}

#[tokio::test]
async fn test_same_source_interceptor_clone_remains_retry_owned() {
    let server = spawn_multi_shot_server(vec![]).await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.send_timeout = Duration::from_secs(5);
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientBuilder::new().create(options).unwrap();
    let flow_token = HttpCancellationToken::new();
    client.add_request_interceptor(HttpRequestInterceptor::new({
        let flow_token = flow_token.clone();
        move |request: &mut HttpRequest| {
            request.set_cancellation_token(flow_token.clone());
            Ok(())
        }
    }));
    let attempts = Arc::new(AtomicUsize::new(0));
    client.add_async_header_injector(AsyncHttpHeaderInjector::new({
        let flow_token = flow_token.clone();
        let attempts = Arc::clone(&attempts);
        move |_headers: &mut HeaderMap| {
            flow_token.cancel();
            attempts.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::pending::<HttpResult<()>>())
        }
    }));

    let request = client
        .request(Method::GET, "/same-source-owner")
        .cancellation_token(flow_token)
        .build();
    let error = client.execute(request).await.unwrap_err();

    assert!(matches!(retry_failure(&error), _));
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(server.finish().await.is_empty());
}
