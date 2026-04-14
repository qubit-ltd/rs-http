/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::{Duration, Instant};

use http::{Method, StatusCode};
use qubit_http::{
    Delay, HttpClientFactory, HttpClientOptions, HttpErrorKind, HttpRetryMethodPolicy,
};
use tokio::time::timeout;

use crate::common::{spawn_multi_shot_server, spawn_one_shot_server, ResponsePlan};

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
    options.retry.delay_strategy = Delay::None;
    let client = HttpClientFactory::new()
        .create_with_options(options)
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
    assert_eq!(response.status, StatusCode::OK);

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
    options.retry.delay_strategy = Delay::None;
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .expect("client should be created");

    let request = client
        .request(Method::GET, "/disable-retry")
        .disable_retry()
        .build();
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
    options.retry.delay_strategy = Delay::None;
    options.retry.method_policy = HttpRetryMethodPolicy::IdempotentOnly;
    let client = HttpClientFactory::new()
        .create_with_options(options)
        .expect("client should be created");

    let request = client
        .request(Method::POST, "/post-method-override")
        .retry_method_policy(HttpRetryMethodPolicy::AllMethods)
        .build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should succeed after retry");
    assert_eq!(response.status, StatusCode::OK);

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
    options.retry.delay_strategy = Delay::None;
    let client = HttpClientFactory::new()
        .create_with_options(options)
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
    assert_eq!(response.status, StatusCode::OK);
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
    options.retry.delay_strategy = Delay::None;
    let client = HttpClientFactory::new()
        .create_with_options(options)
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
    assert_eq!(response.status, StatusCode::OK);
    assert!(
        elapsed >= Duration::from_millis(900),
        "elapsed={elapsed:?} should reflect Retry-After waiting on 503"
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
}
