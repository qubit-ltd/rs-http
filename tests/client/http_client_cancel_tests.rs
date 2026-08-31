// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error as _;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;
use std::time::Duration;

use futures_util::StreamExt;
use http::Method;
use http::StatusCode;
use qubit_http::AsyncHttpHeaderInjector;
use qubit_http::HttpClientFactory;
use qubit_http::HttpClientOptions;
use qubit_http::HttpError;
use qubit_http::HttpErrorKind;
use qubit_http::HttpResponseInterceptor;
use qubit_http::RetryCancellationToken;
use qubit_http::RetryHint;
use qubit_retry::BackoffPolicy;
use qubit_retry::RetryCancellationPhase;
use qubit_retry::RetryError;
use qubit_retry::RetryFailure;
use tokio::sync::Notify;
use tokio::time::timeout;

use crate::common::ResponseChunk;
use crate::common::ResponsePlan;
use crate::common::spawn_blocked_one_shot_server;
use crate::common::spawn_multi_shot_server;
use crate::common::spawn_one_shot_server;

/// Wakes a deterministic test barrier when the polled HTTP future is ready to
/// make progress.
struct NotifyWake {
    notify: Notify,
}

impl Wake for NotifyWake {
    fn wake(self: Arc<Self>) {
        self.notify.notify_one();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.notify.notify_one();
    }
}

#[test]
fn test_cancelled_error_semantics() {
    let error = HttpError::cancelled("request cancelled by caller");
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(error.retry_hint(), RetryHint::NonRetryable);
    assert!(error.message.contains("cancelled"));
}

/// Returns the structured retry terminal retained as an HTTP error source.
fn retry_failure(error: &HttpError) -> &RetryFailure<HttpError> {
    error
        .source()
        .and_then(|source| source.downcast_ref::<RetryError<HttpError>>())
        .expect("retry cancellation should retain RetryError as its source")
        .failure()
}

#[tokio::test]
async fn test_execute_request_with_pre_cancelled_token_returns_cancelled_error() {
    let server = spawn_multi_shot_server(vec![]).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = RetryCancellationToken::new();
    token.cancel();
    let request = client
        .request(Method::GET, "/pre-cancelled")
        .cancellation_token(token)
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("request should be cancelled");
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(error.message.contains("cancelled"));
    assert!(matches!(
        retry_failure(&error),
        RetryFailure::Cancelled {
            phase: RetryCancellationPhase::BeforeAttempt,
            ..
        }
    ));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert!(captured.is_empty());
}

#[tokio::test]
async fn test_execute_request_with_pre_cancelled_token_skips_request_interceptors() {
    let server = spawn_multi_shot_server(vec![]).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let interceptor_calls = Arc::new(AtomicUsize::new(0));
    let interceptor_calls_clone = Arc::clone(&interceptor_calls);
    client.add_request_interceptor(qubit_http::HttpRequestInterceptor::new(
        move |_request: &mut qubit_http::HttpRequest| {
            interceptor_calls_clone.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
    ));

    let token = RetryCancellationToken::new();
    token.cancel();
    let request = client
        .request(Method::GET, "/pre-cancelled-interceptor")
        .query_param("trace", "yes")
        .cancellation_token(token)
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("request should be cancelled");

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(interceptor_calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        error
            .url
            .as_ref()
            .expect("cancelled error should include request URL")
            .query(),
        Some("trace=yes")
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert!(captured.is_empty());
}

#[tokio::test]
async fn test_execute_request_cancelled_by_interceptor_stops_before_send() {
    let server = spawn_multi_shot_server(vec![]).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = RetryCancellationToken::new();
    let interceptor_token = token.clone();
    client.add_request_interceptor(qubit_http::HttpRequestInterceptor::new(
        move |_request: &mut qubit_http::HttpRequest| {
            interceptor_token.cancel();
            Ok(())
        },
    ));

    let request = client
        .request(Method::GET, "/cancelled-by-interceptor")
        .cancellation_token(token)
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("interceptor cancellation should stop before send");

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(error.message.contains("before sending"));
    assert_eq!(
        error
            .url
            .as_ref()
            .expect("cancelled error should include request URL")
            .path(),
        "/cancelled-by-interceptor"
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert!(captured.is_empty());
}

#[tokio::test]
async fn test_execute_request_can_be_cancelled_while_preparing_async_headers() {
    let server = spawn_multi_shot_server(vec![]).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_secs(5);
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let token = RetryCancellationToken::new();
    let injector_token = token.clone();
    let attempt_calls = Arc::new(AtomicUsize::new(0));
    let injector_attempt_calls = Arc::clone(&attempt_calls);
    client.add_request_interceptor(qubit_http::HttpRequestInterceptor::new(
        |request: &mut qubit_http::HttpRequest| {
            assert!(
                request.cancellation_token().is_some(),
                "retry attempts must retain the shared public token"
            );
            Ok(())
        },
    ));
    client.add_async_header_injector(AsyncHttpHeaderInjector::new(move |_headers: &mut http::HeaderMap| {
        let injector_token = injector_token.clone();
        injector_attempt_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            injector_token.cancel();
            std::future::pending::<qubit_http::HttpResult<()>>().await
        })
    }));

    let request = client
        .request(Method::GET, "/cancel-preparing-headers")
        .cancellation_token(token)
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("request should be cancelled while preparing headers");

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(matches!(
        retry_failure(&error),
        RetryFailure::Cancelled {
            phase: RetryCancellationPhase::Attempt,
            ..
        }
    ));
    assert_eq!(attempt_calls.load(Ordering::SeqCst), 1);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert!(captured.is_empty());
}

#[tokio::test]
async fn test_execute_request_can_be_cancelled_while_reading_response_body() {
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
    options.timeouts.read_timeout = Duration::from_secs(5);
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = RetryCancellationToken::new();
    let request = client
        .request(Method::GET, "/cancel-reading")
        .cancellation_token(token.clone())
        .build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should start");
    let body = response.bytes();
    tokio::pin!(body);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(
        body.as_mut().poll(&mut context).is_pending(),
        "partial response body should wait for its remaining bytes"
    );
    token.cancel();
    let error = match body.as_mut().poll(&mut context) {
        Poll::Ready(Err(error)) => error,
        Poll::Ready(Ok(_)) => panic!("cancelled body read must not succeed"),
        Poll::Pending => panic!("ready body cancellation must finish the read"),
    };
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(error.message.contains("cancelled"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/cancel-reading");
}

#[tokio::test]
async fn test_execute_request_can_be_cancelled_while_reading_status_error_preview() {
    let mut server = spawn_blocked_one_shot_server(ResponsePlan::Chunked {
        status: 503,
        headers: vec![],
        chunks: vec![
            ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"partial".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_secs(1),
                bytes: b"later".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(5);
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = RetryCancellationToken::new();
    let request = client
        .request(Method::GET, "/cancel-status-preview")
        .query_param("phase", "preview")
        .cancellation_token(token.clone())
        .build();
    let execution = client.execute(request);
    tokio::pin!(execution);
    tokio::select! {
        () = server.wait_until_request_received() => {}
        result = &mut execution => {
            panic!("request completed before server response: {result:?}");
        }
    }
    let notify_wake = Arc::new(NotifyWake { notify: Notify::new() });
    let waker = Waker::from(Arc::clone(&notify_wake));
    let mut context = Context::from_waker(&waker);
    assert!(
        execution.as_mut().poll(&mut context).is_pending(),
        "request should wait for the blocked response"
    );
    server.allow_response();
    server.wait_until_response_started().await;
    timeout(Duration::from_secs(3), notify_wake.notify.notified())
        .await
        .expect("response readiness did not wake execution");
    assert!(
        execution.as_mut().poll(&mut context).is_pending(),
        "status response should enter preview body read"
    );
    token.cancel();
    let error = timeout(Duration::from_secs(3), &mut execution)
        .await
        .expect("execute timed out")
        .expect_err("status error preview should be cancelled");

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(error.status, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert!(error.message.contains("status error response body preview"));
    assert_eq!(
        error
            .url
            .as_ref()
            .expect("cancelled error should include request URL")
            .query(),
        Some("phase=preview")
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/cancel-status-preview?phase=preview");
}

#[tokio::test(start_paused = true)]
async fn test_execute_retry_sleep_can_be_cancelled() {
    let mut options = HttpClientOptions::default();
    options
        .set_base_url("http://127.0.0.1:9")
        .expect("test base URL should parse");
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.backoff = BackoffPolicy::fixed(Duration::from_secs(5));
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let attempt_calls = Arc::new(AtomicUsize::new(0));
    client.add_request_interceptor(qubit_http::HttpRequestInterceptor::new({
        let attempt_calls = Arc::clone(&attempt_calls);
        move |_request: &mut qubit_http::HttpRequest| {
            attempt_calls.fetch_add(1, Ordering::SeqCst);
            Err(HttpError::transport("deterministic retry failure"))
        }
    }));

    let token = RetryCancellationToken::new();
    let request = client
        .request(Method::GET, "/cancel-retry-sleep")
        .cancellation_token(token.clone())
        .build();
    let future = client.execute(request);
    tokio::pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(future.as_mut().poll(&mut context).is_pending());

    token.cancel();
    tokio::time::advance(Duration::from_secs(5)).await;
    let error = match future.as_mut().poll(&mut context) {
        Poll::Ready(Err(error)) => error,
        Poll::Ready(Ok(_)) => panic!("cancelled backoff must not retry"),
        Poll::Pending => panic!("ready cancellation must finish the request"),
    };

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(matches!(
        retry_failure(&error),
        RetryFailure::Cancelled {
            phase: RetryCancellationPhase::Backoff,
            last_failure: Some(_),
            ..
        }
    ));
    assert_eq!(attempt_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_execute_retry_success_wins_same_poll_cancellation() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"success".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let token = RetryCancellationToken::new();
    client.add_response_interceptor(HttpResponseInterceptor::new({
        let token = token.clone();
        move |_response: &mut qubit_http::HttpResponseInterceptorContext| {
            token.cancel();
            Ok(())
        }
    }));

    let request = client
        .request(Method::GET, "/success-cancel-race")
        .cancellation_token(token)
        .build();
    let response = client
        .execute(request)
        .await
        .expect("completed HTTP response must win same-poll cancellation");

    assert_eq!(response.status(), StatusCode::OK);
    let captured = server.finish().await;
    assert_eq!(captured.target, "/success-cancel-race");
}

#[tokio::test]
async fn test_retry_interceptor_request_clone_keeps_direct_cancellation() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"success".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut retry_client = HttpClientFactory::new()
        .create(options)
        .expect("retry client should be created");
    let saved_request = Arc::new(Mutex::new(None));
    retry_client.add_request_interceptor(qubit_http::HttpRequestInterceptor::new({
        let saved_request = Arc::clone(&saved_request);
        move |request: &mut qubit_http::HttpRequest| {
            *saved_request.lock().expect("saved request lock") = Some(request.clone());
            Ok(())
        }
    }));

    let token = RetryCancellationToken::new();
    let request = retry_client
        .request(Method::GET, "/clone-for-direct-execution")
        .cancellation_token(token.clone())
        .build();
    retry_client
        .execute(request)
        .await
        .expect("initial retry-enabled request should succeed");
    let captured = server.finish().await;
    assert_eq!(captured.target, "/clone-for-direct-execution");

    let cloned_request = saved_request
        .lock()
        .expect("saved request lock")
        .take()
        .expect("interceptor should save one request clone");
    token.cancel();
    let direct_client = HttpClientFactory::new()
        .create(HttpClientOptions::default())
        .expect("direct client should be created");
    let error = direct_client
        .execute(cloned_request)
        .await
        .expect_err("saved clone should retain direct cancellation behavior");
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
}

#[tokio::test]
async fn test_retry_interceptor_replacement_token_reaches_response_body() {
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
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let replacement_token = RetryCancellationToken::new();
    client.add_request_interceptor(qubit_http::HttpRequestInterceptor::new({
        let replacement_token = replacement_token.clone();
        move |request: &mut qubit_http::HttpRequest| {
            request.set_cancellation_token(replacement_token.clone());
            Ok(())
        }
    }));

    let flow_token = RetryCancellationToken::new();
    let request = client
        .request(Method::GET, "/replacement-token-body")
        .cancellation_token(flow_token.clone())
        .build();
    let mut response = client
        .execute(request)
        .await
        .expect("retry-enabled request should return response headers");
    let body = response.bytes();
    tokio::pin!(body);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(body.as_mut().poll(&mut context).is_pending());

    replacement_token.cancel();
    let error = match body.as_mut().poll(&mut context) {
        Poll::Ready(Err(error)) => error,
        Poll::Ready(Ok(_)) => {
            panic!("replacement token cancellation must stop body read")
        }
        Poll::Pending => panic!("replacement token cancellation must be ready"),
    };
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(!flow_token.is_cancelled());

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/replacement-token-body");
}

#[tokio::test]
async fn test_retry_multi_attempt_response_uses_success_replacement_token() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 503,
            headers: vec![],
            body: b"retry".to_vec(),
        },
        ResponsePlan::PartialThenDelay {
            status: 200,
            headers: vec![],
            total_length: 16,
            prefix: b"abc".to_vec(),
            delay: Duration::from_secs(2),
        },
    ])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let first_token = RetryCancellationToken::new();
    let success_token = RetryCancellationToken::new();
    let interceptor_calls = Arc::new(AtomicUsize::new(0));
    client.add_request_interceptor(qubit_http::HttpRequestInterceptor::new({
        let first_token = first_token.clone();
        let success_token = success_token.clone();
        let interceptor_calls = Arc::clone(&interceptor_calls);
        move |request: &mut qubit_http::HttpRequest| {
            let attempt = interceptor_calls.fetch_add(1, Ordering::SeqCst);
            let token = if attempt == 0 {
                first_token.clone()
            } else {
                success_token.clone()
            };
            request.set_cancellation_token(token);
            Ok(())
        }
    }));

    let flow_token = RetryCancellationToken::new();
    let request = client
        .request(Method::GET, "/multi-attempt-replacement")
        .cancellation_token(flow_token.clone())
        .build();
    let mut response = client
        .execute(request)
        .await
        .expect("second attempt should return a response");
    assert_eq!(interceptor_calls.load(Ordering::SeqCst), 2);
    let body = response.bytes();
    tokio::pin!(body);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(body.as_mut().poll(&mut context).is_pending());

    first_token.cancel();
    assert!(
        body.as_mut().poll(&mut context).is_pending(),
        "failed-attempt replacement must not leak into final response"
    );
    success_token.cancel();
    let error = match body.as_mut().poll(&mut context) {
        Poll::Ready(Err(error)) => error,
        Poll::Ready(Ok(_)) => {
            panic!("success-attempt token cancellation must stop body read")
        }
        Poll::Pending => {
            panic!("success-attempt token cancellation must be ready")
        }
    };
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(!flow_token.is_cancelled());

    let captured = server.finish().await;
    assert_eq!(captured.len(), 2);
}

#[tokio::test]
async fn test_retry_multi_attempt_failed_clear_does_not_leak_to_response() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 503,
            headers: vec![],
            body: b"retry".to_vec(),
        },
        ResponsePlan::PartialThenDelay {
            status: 200,
            headers: vec![],
            total_length: 16,
            prefix: b"abc".to_vec(),
            delay: Duration::from_secs(2),
        },
    ])
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let interceptor_calls = Arc::new(AtomicUsize::new(0));
    client.add_request_interceptor(qubit_http::HttpRequestInterceptor::new({
        let interceptor_calls = Arc::clone(&interceptor_calls);
        move |request: &mut qubit_http::HttpRequest| {
            if interceptor_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                request.clear_cancellation_token();
            }
            Ok(())
        }
    }));

    let flow_token = RetryCancellationToken::new();
    let request = client
        .request(Method::GET, "/multi-attempt-clear")
        .cancellation_token(flow_token.clone())
        .build();
    let mut response = client
        .execute(request)
        .await
        .expect("second attempt should return a response");
    assert_eq!(interceptor_calls.load(Ordering::SeqCst), 2);
    let body = response.bytes();
    tokio::pin!(body);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(body.as_mut().poll(&mut context).is_pending());

    flow_token.cancel();
    let error = match body.as_mut().poll(&mut context) {
        Poll::Ready(Err(error)) => error,
        Poll::Ready(Ok(_)) => {
            panic!("successful unchanged attempt must restore flow token")
        }
        Poll::Pending => panic!("flow token cancellation must be ready"),
    };
    assert_eq!(error.kind, HttpErrorKind::Cancelled);

    let captured = server.finish().await;
    assert_eq!(captured.len(), 2);
}

#[tokio::test]
async fn test_retry_interceptor_replacement_token_controls_attempt_io() {
    let server = spawn_multi_shot_server(vec![]).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_secs(5);
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let replacement_token = RetryCancellationToken::new();
    client.add_request_interceptor(qubit_http::HttpRequestInterceptor::new({
        let replacement_token = replacement_token.clone();
        move |request: &mut qubit_http::HttpRequest| {
            request.set_cancellation_token(replacement_token.clone());
            Ok(())
        }
    }));
    let attempt_calls = Arc::new(AtomicUsize::new(0));
    client.add_async_header_injector(AsyncHttpHeaderInjector::new({
        let replacement_token = replacement_token.clone();
        let attempt_calls = Arc::clone(&attempt_calls);
        move |_headers: &mut http::HeaderMap| {
            replacement_token.cancel();
            attempt_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::pending::<qubit_http::HttpResult<()>>())
        }
    }));

    let flow_token = RetryCancellationToken::new();
    let request = client
        .request(Method::GET, "/replacement-token-attempt")
        .cancellation_token(flow_token.clone())
        .build();
    let error = client
        .execute(request)
        .await
        .expect_err("replacement token should cancel attempt I/O");
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert_eq!(attempt_calls.load(Ordering::SeqCst), 1);
    assert!(!flow_token.is_cancelled());

    let captured = server.finish().await;
    assert!(captured.is_empty());
}

#[tokio::test]
async fn test_retry_interceptor_cleared_token_is_not_restored_on_response() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"success".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    client.add_request_interceptor(qubit_http::HttpRequestInterceptor::new(
        |request: &mut qubit_http::HttpRequest| {
            request.clear_cancellation_token();
            Ok(())
        },
    ));

    let flow_token = RetryCancellationToken::new();
    let request = client
        .request(Method::GET, "/cleared-token-body")
        .cancellation_token(flow_token.clone())
        .build();
    let mut response = client
        .execute(request)
        .await
        .expect("retry-enabled request should succeed");
    flow_token.cancel();
    let body = response
        .bytes()
        .await
        .expect("cleared token must not be restored on response");
    assert_eq!(body, b"success".as_slice());

    let captured = server.finish().await;
    assert_eq!(captured.target, "/cleared-token-body");
}

#[tokio::test]
async fn test_retry_success_propagates_flow_token_to_response_body() {
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
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    let token = RetryCancellationToken::new();
    let request = client
        .request(Method::GET, "/flow-token-body")
        .cancellation_token(token.clone())
        .build();
    let mut response = client
        .execute(request)
        .await
        .expect("retry-enabled request should return response headers");
    let body = response.bytes();
    tokio::pin!(body);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(body.as_mut().poll(&mut context).is_pending());

    token.cancel();
    let error = match body.as_mut().poll(&mut context) {
        Poll::Ready(Err(error)) => error,
        Poll::Ready(Ok(_)) => {
            panic!("flow token cancellation must stop body read")
        }
        Poll::Pending => panic!("flow token cancellation must be ready"),
    };
    assert_eq!(error.kind, HttpErrorKind::Cancelled);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/flow-token-body");
}

#[tokio::test]
async fn test_execute_request_can_be_cancelled_while_sending() {
    let mut server = spawn_one_shot_server(ResponsePlan::DelayedStart {
        delay: Duration::from_secs(2),
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.write_timeout = Duration::from_secs(5);
    options.timeouts.read_timeout = Duration::from_secs(5);
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = RetryCancellationToken::new();
    let request = client
        .request(Method::GET, "/cancel-sending")
        .cancellation_token(token.clone())
        .build();
    let execution = client.execute(request);
    tokio::pin!(execution);
    tokio::select! {
        () = server.wait_until_request_received() => {}
        result = &mut execution => {
            panic!("send completed before cancellation: {result:?}");
        }
    }
    token.cancel();
    let error = timeout(Duration::from_secs(3), &mut execution)
        .await
        .expect("execute timed out")
        .expect_err("request should be cancelled while sending");
    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(error.message.contains("sending"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/cancel-sending");
}

#[tokio::test]
async fn test_execute_stream_body_can_be_cancelled_after_first_chunk() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
        chunks: vec![
            ResponseChunk {
                delay: Duration::ZERO,
                bytes: b"first".to_vec(),
            },
            ResponseChunk {
                delay: Duration::from_secs(2),
                bytes: b"second".to_vec(),
            },
        ],
        finish: true,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(5);
    options.retry.enabled = true;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = RetryCancellationToken::new();
    let request = client
        .request(Method::GET, "/cancel-stream")
        .cancellation_token(token.clone())
        .build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should start");

    let mut stream = response.stream().expect("stream body should be available");
    let first = stream
        .next()
        .await
        .expect("first stream item should exist")
        .expect("first stream item should be ok");
    assert_eq!(first, b"first".as_slice());

    token.cancel();

    let cancelled = stream
        .next()
        .await
        .expect("second stream item should exist")
        .expect_err("second stream item should be cancelled");
    assert_eq!(cancelled.kind, HttpErrorKind::Cancelled);
    assert!(cancelled.message.contains("cancelled"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/cancel-stream");
}

#[tokio::test]
async fn test_sse_messages_reports_pre_cancelled_stream_before_reading_body() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![],
        finish: false,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(5);
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = RetryCancellationToken::new();
    let request = client
        .request(Method::GET, "/cancel-sse-events-before-read")
        .cancellation_token(token.clone())
        .build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should start");
    token.cancel();

    let mut events = response.sse_messages();
    let error = events
        .next()
        .await
        .expect("pre-cancelled SSE message stream should yield one error")
        .expect_err("SSE message stream should fail before reading body");

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(error.message.contains("before reading response body"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/cancel-sse-events-before-read");
}

#[tokio::test]
async fn test_sse_chunks_reports_pre_cancelled_stream_before_reading_body() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![("Content-Type".to_string(), "text/event-stream".to_string())],
        chunks: vec![],
        finish: false,
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_secs(5);
    let client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");

    let token = RetryCancellationToken::new();
    let request = client
        .request(Method::GET, "/cancel-sse-chunks-before-read")
        .cancellation_token(token.clone())
        .build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should start");
    token.cancel();

    let mut chunks = response.sse_chunks::<serde_json::Value>();
    let error = chunks
        .next()
        .await
        .expect("pre-cancelled SSE chunk stream should yield one error")
        .expect_err("SSE chunk stream should fail before reading body");

    assert_eq!(error.kind, HttpErrorKind::Cancelled);
    assert!(error.message.contains("before reading response body"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/cancel-sse-chunks-before-read");
}
