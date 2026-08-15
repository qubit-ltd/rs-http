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

use http::Method;
use qubit_http::AsyncHttpHeaderInjector;
use qubit_http::HttpClientFactory;
use qubit_http::HttpClientOptions;
use qubit_http::HttpError;
use qubit_http::RetryCancellationToken;
use qubit_retry::BackoffPolicy;
use qubit_retry::RetryCancellationPhase;
use qubit_retry::RetryError;
use qubit_retry::RetryFailure;

use crate::common::spawn_multi_shot_server;

/// Returns the structured retry terminal chained by an HTTP cancellation.
fn retry_failure(error: &HttpError) -> &RetryFailure<HttpError> {
    error
        .source()
        .and_then(|source| source.downcast_ref::<RetryError<HttpError>>())
        .expect("retry cancellation should retain RetryError")
        .failure()
}

#[tokio::test]
async fn test_same_source_interceptor_clone_remains_retry_owned() {
    let server = spawn_multi_shot_server(vec![]).await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_secs(5);
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientFactory::new().create(options).unwrap();
    let flow_token = RetryCancellationToken::new();
    client.add_request_interceptor(qubit_http::HttpRequestInterceptor::new({
        let flow_token = flow_token.clone();
        move |request: &mut qubit_http::HttpRequest| {
            request.set_cancellation_token(flow_token.clone());
            Ok(())
        }
    }));
    let attempts = Arc::new(AtomicUsize::new(0));
    client.add_async_header_injector(AsyncHttpHeaderInjector::new({
        let flow_token = flow_token.clone();
        let attempts = Arc::clone(&attempts);
        move |_headers: &mut http::HeaderMap| {
            flow_token.cancel();
            attempts.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::pending::<qubit_http::HttpResult<()>>())
        }
    }));

    let request = client
        .request(Method::GET, "/same-source-owner")
        .cancellation_token(flow_token)
        .build();
    let error = client.execute(request).await.unwrap_err();

    assert!(matches!(
        retry_failure(&error),
        RetryFailure::Cancelled {
            phase: RetryCancellationPhase::Attempt,
            ..
        }
    ));
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(server.finish().await.is_empty());
}
