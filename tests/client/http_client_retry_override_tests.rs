/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::sync::atomic::{
    AtomicBool,
    AtomicUsize,
    Ordering,
};
use std::sync::Arc;
use std::time::{
    Duration,
    Instant,
};

use futures_util::StreamExt;
use http::{
    Method,
    StatusCode,
};
use qubit_http::{
    HttpClientFactory,
    HttpClientOptions,
    HttpErrorKind,
    HttpRetryMethodPolicy,
    RetryDelay,
};
use tokio::time::timeout;

use crate::common::{
    spawn_multi_shot_server,
    spawn_one_shot_server,
    ResponseChunk,
    ResponsePlan,
};

#[tokio::test]
async fn test_request_retry_override_force_enable_and_all_methods_for_post() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 500,
            headers: vec![],
            body: b"server-error".to_vec(),
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
    options.retry.enabled = false;
    options.retry.max_attempts = 2;
    options.retry.delay_strategy = RetryDelay::None;
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let request = client
        .request(Method::POST, "/force-enable")
        .force_retry()
        .retry_method_policy(HttpRetryMethodPolicy::AllMethods)
        .build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should succeed after retry");
    assert_eq!(response.status(), StatusCode::OK);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].method, "POST");
    assert_eq!(captured[1].method, "POST");
}

#[tokio::test]
async fn test_request_retry_override_disable_retry_skips_client_retry_policy() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 503,
        headers: vec![],
        body: b"service unavailable".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.delay_strategy = RetryDelay::None;
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let request = client.request(Method::GET, "/disable-retry").disable_retry().build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("request should fail without retry");
    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::SERVICE_UNAVAILABLE));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/disable-retry");
}

#[tokio::test]
async fn test_request_retry_override_method_policy_allows_post_without_global_override() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 500,
            headers: vec![],
            body: b"server-error".to_vec(),
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
    options.retry.method_policy = HttpRetryMethodPolicy::IdempotentOnly;
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let request = client
        .request(Method::POST, "/post-method-override")
        .retry_method_policy(HttpRetryMethodPolicy::AllMethods)
        .build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should succeed after retry");
    assert_eq!(response.status(), StatusCode::OK);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
}

#[tokio::test]
async fn test_request_retry_override_honor_retry_after_waits_before_retrying() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 429,
            headers: vec![("Retry-After".to_string(), "1".to_string())],
            body: b"too many requests".to_vec(),
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
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let request = client
        .request(Method::GET, "/retry-after")
        .honor_retry_after(true)
        .build();
    let start = Instant::now();
    let response = timeout(Duration::from_secs(4), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should succeed after retry");
    let elapsed = start.elapsed();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        elapsed >= Duration::from_millis(900),
        "elapsed={elapsed:?} should reflect Retry-After waiting"
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
}

#[tokio::test]
async fn test_request_retry_override_honor_retry_after_waits_before_retrying_on_503() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 503,
            headers: vec![("Retry-After".to_string(), "1".to_string())],
            body: b"service unavailable".to_vec(),
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
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let request = client
        .request(Method::GET, "/retry-after-503")
        .honor_retry_after(true)
        .build();
    let start = Instant::now();
    let response = timeout(Duration::from_secs(4), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should succeed after retry");
    let elapsed = start.elapsed();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        elapsed >= Duration::from_millis(900),
        "elapsed={elapsed:?} should reflect Retry-After waiting on 503"
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
}

#[tokio::test]
async fn test_request_retry_override_honor_retry_after_waits_before_body_stream_retrying() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 429,
            headers: vec![("Retry-After".to_string(), "1".to_string())],
            body: b"too many requests".to_vec(),
        },
        ResponsePlan::Chunked {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
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
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let request = client
        .request(Method::GET, "/stream-retry-after")
        .honor_retry_after(true)
        .build();
    let start = Instant::now();
    let mut response = timeout(Duration::from_secs(4), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should succeed after retry");
    let elapsed = start.elapsed();
    let body = response
        .stream()
        .expect("stream body should be available")
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("stream body should decode");
    assert_eq!(body[0], b"stream-ok".as_slice());
    assert!(
        elapsed >= Duration::from_millis(900),
        "elapsed={elapsed:?} should reflect Retry-After waiting before stream retry"
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
}

#[tokio::test]
async fn test_request_retry_override_honor_retry_after_without_header_does_not_add_delay() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 503,
            headers: vec![],
            body: b"service unavailable".to_vec(),
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
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let request = client
        .request(Method::GET, "/retry-after-missing-header")
        .honor_retry_after(true)
        .build();
    let start = Instant::now();
    let response = timeout(Duration::from_secs(4), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should succeed after retry");
    let elapsed = start.elapsed();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        elapsed < Duration::from_millis(700),
        "elapsed={elapsed:?} should not include extra Retry-After delay when header is missing"
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn test_request_retry_override_honor_retry_after_does_not_block_runtime_thread() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 429,
            headers: vec![("Retry-After".to_string(), "1".to_string())],
            body: b"too many requests".to_vec(),
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
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let tick_count = Arc::new(AtomicUsize::new(0));
    let stop_ticker = Arc::new(AtomicBool::new(false));
    let tick_count_for_task = Arc::clone(&tick_count);
    let stop_ticker_for_task = Arc::clone(&stop_ticker);
    let ticker = tokio::spawn(async move {
        while !stop_ticker_for_task.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(50)).await;
            tick_count_for_task.fetch_add(1, Ordering::Relaxed);
        }
    });

    let request = client
        .request(Method::GET, "/retry-after-async-friendly")
        .honor_retry_after(true)
        .build();
    let response = timeout(Duration::from_secs(4), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should succeed after retry");
    assert_eq!(response.status(), StatusCode::OK);

    stop_ticker.store(true, Ordering::Relaxed);
    timeout(Duration::from_secs(2), ticker)
        .await
        .expect("ticker join timed out")
        .expect("ticker task panicked");
    assert!(
        tick_count.load(Ordering::Relaxed) >= 5,
        "ticker should keep running while honoring Retry-After without blocking runtime"
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
}
