// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public behavior coverage for private successful-attempt handoff.

use std::future::Future;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;
use std::time::Duration;

use http::Method;
use qubit_http::HttpClientFactory;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use qubit_http::RetryCancellationToken;
use qubit_retry::BackoffPolicy;
use tokio::time::timeout;

use crate::common::ResponsePlan;
use crate::common::spawn_one_shot_server;

#[tokio::test]
async fn test_retry_success_hands_flow_token_to_response() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 200,
        headers: vec![],
        total_length: 16,
        prefix: b"abc".to_vec(),
        delay: Duration::from_secs(2),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    let client = HttpClientFactory::new().create(options).unwrap();
    let token = RetryCancellationToken::new();
    let request = client
        .request(Method::GET, "/response-token-handoff")
        .cancellation_token(token.clone())
        .build();
    let mut response = client.execute(request).await.unwrap();
    let body = response.bytes();
    tokio::pin!(body);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(body.as_mut().poll(&mut context).is_pending());

    token.cancel();
    let error = match body.as_mut().poll(&mut context) {
        Poll::Ready(Err(error)) => error,
        Poll::Ready(Ok(_)) => {
            panic!("cancelled response body must not succeed")
        }
        Poll::Pending => panic!("response token cancellation must be ready"),
    };

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/response-token-handoff");
}
