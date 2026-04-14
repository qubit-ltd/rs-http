/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::Duration;

use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions, HttpErrorKind, RetryHint};
use tokio::time::timeout;

use crate::common::{spawn_one_shot_server, ResponsePlan};

#[tokio::test]
async fn test_client_level_request_timeout_triggers_timeout_classification() {
    let server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_millis(250),
        status: 200,
        headers: vec![],
        body: b"late".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_millis(80));

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client.request(Method::GET, "/request-timeout").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::RequestTimeout);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}

#[tokio::test]
async fn test_request_level_timeout_overrides_client_level_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_millis(250),
        status: 200,
        headers: vec![],
        body: b"late".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(5));

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client
        .request(Method::GET, "/request-timeout-override")
        .timeout(Duration::from_millis(80))
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::RequestTimeout);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}

#[tokio::test]
async fn test_timeout_classification_is_retryable_in_deterministic_path() {
    let server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_millis(250),
        status: 200,
        headers: vec![],
        body: b"late".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_millis(80));

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client
        .request(Method::GET, "/deterministic-timeout")
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::RequestTimeout);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}

#[tokio::test]
async fn test_request_timeout_during_body_read_is_classified_as_read_timeout() {
    let server = spawn_one_shot_server(ResponsePlan::PartialThenDelay {
        status: 200,
        headers: vec![],
        total_length: 16,
        prefix: b"abc".to_vec(),
        delay: Duration::from_millis(250),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_millis(80));

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client
        .request(Method::GET, "/request-timeout-read-phase")
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::ReadTimeout);
    assert_eq!(error.retry_hint(), RetryHint::Retryable);
}
