// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error as StdError;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use futures_util::StreamExt;
use http::Method;
use http::StatusCode;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpError;
use qubit_http::HttpErrorKind;
use qubit_http::HttpRequest;
use qubit_http::HttpRequestBody;
use qubit_http::HttpRequestInterceptor;
use qubit_http::HttpRequestRetryOverride;
use qubit_http::HttpRetryMethodPolicy;
use qubit_retry::BackoffPolicy;
use qubit_retry::RetryError;
use tokio::spawn;
use tokio::test as tokio_test;
use tokio::time::sleep;
use tokio::time::timeout;

use crate::common::ResponseChunk;
use crate::common::ResponsePlan;
use crate::common::spawn_multi_shot_server;
use crate::common::spawn_one_shot_server;

#[tokio_test]
async fn test_text_body_retry_without_interceptors_sends_identical_bytes() {
    for explicit_content_type in [false, true] {
        let server = spawn_multi_shot_server(vec![
            ResponsePlan::Immediate {
                status: 503,
                headers: vec![],
                body: b"retry".to_vec(),
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
        options.retry.backoff = BackoffPolicy::immediate();
        let client = HttpClientBuilder::new()
            .options(options)
            .build()
            .expect("client should be created");
        let mut builder = client
            .request(Method::POST, "/text-retry")
            .text_body("shared-retry-payload")
            .retry_method_policy(HttpRetryMethodPolicy::AllMethods)
            .force_retry();
        if explicit_content_type {
            builder = builder
                .header("Content-Type", "application/custom")
                .expect("header should parse");
        }
        let request = builder.build();
        let response = timeout(Duration::from_secs(3), client.execute(request))
            .await
            .expect("execute timed out")
            .expect("retry should succeed");
        assert_eq!(response.status(), StatusCode::OK);
        let captured = timeout(Duration::from_secs(3), server.finish())
            .await
            .expect("server timed out");
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0].body, b"shared-retry-payload");
        assert_eq!(captured[1].body, b"shared-retry-payload");
        let expected = if explicit_content_type {
            "application/custom"
        } else {
            "text/plain; charset=utf-8"
        };
        assert_eq!(
            captured[0].headers.get("content-type").map(String::as_str),
            Some(expected)
        );
        assert_eq!(
            captured[1].headers.get("content-type").map(String::as_str),
            Some(expected)
        );
    }
}

#[tokio_test]
async fn test_text_body_retry_recreates_mutable_attempt_from_snapshot() {
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
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let observed_bodies = Arc::new(Mutex::new(Vec::new()));
    let interceptor_bodies = Arc::clone(&observed_bodies);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |request: &mut HttpRequest| {
        let HttpRequestBody::Text(body) = request.body() else {
            panic!("text request body should remain observable to the interceptor");
        };
        interceptor_bodies
            .lock()
            .expect("body observations should be unlocked")
            .push(body.clone());
        request.set_body(HttpRequestBody::Text("attempt-local-text".to_string()));
        Ok(())
    }));

    let request = client
        .request(Method::POST, "/text-body-retry")
        .text_body("original-text")
        .retry_method_policy(HttpRetryMethodPolicy::AllMethods)
        .force_retry()
        .build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should succeed after retry");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        *observed_bodies.lock().expect("body observations should be unlocked"),
        vec!["original-text", "original-text"]
    );

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].body, b"attempt-local-text");
    assert_eq!(captured[1].body, b"attempt-local-text");
}

#[tokio_test]
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
    options.retry.backoff = BackoffPolicy::immediate();
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
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

#[tokio_test]
async fn test_force_retry_does_not_override_default_method_policy() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 503,
        headers: vec![],
        body: b"service unavailable".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = false;
    options.retry.max_attempts = 2;
    options.retry.backoff = BackoffPolicy::immediate();
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let request = client.request(Method::POST, "/force-only").force_retry().build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("POST should not retry under the default method policy");
    assert_eq!(error.kind, HttpErrorKind::Status);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.method, "POST");
}

#[tokio_test]
async fn test_force_retry_does_not_override_single_max_attempt() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 503,
        headers: vec![],
        body: b"service unavailable".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = false;
    options.retry.max_attempts = 1;
    options.retry.method_policy = HttpRetryMethodPolicy::AllMethods;
    options.retry.backoff = BackoffPolicy::immediate();
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let request = client.request(Method::POST, "/single-attempt").force_retry().build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("retry should not run with max_attempts set to one");
    assert_eq!(error.kind, HttpErrorKind::Status);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.method, "POST");
}

#[tokio_test]
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
    options.retry.backoff = BackoffPolicy::immediate();
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
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

#[tokio_test]
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
    options.retry.backoff = BackoffPolicy::immediate();
    options.retry.method_policy = HttpRetryMethodPolicy::IdempotentOnly;
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
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

#[tokio_test]
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
    options.retry.backoff = BackoffPolicy::immediate();
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
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

#[tokio_test]
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
    options.retry.backoff = BackoffPolicy::immediate();
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
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

#[tokio_test]
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
    options.retry.backoff = BackoffPolicy::immediate();
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
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

#[tokio_test]
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
    options.retry.backoff = BackoffPolicy::immediate();
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
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

#[tokio_test(flavor = "current_thread")]
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
    options.retry.backoff = BackoffPolicy::immediate();
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let tick_count = Arc::new(AtomicUsize::new(0));
    let stop_ticker = Arc::new(AtomicBool::new(false));
    let tick_count_for_task = Arc::clone(&tick_count);
    let stop_ticker_for_task = Arc::clone(&stop_ticker);
    let ticker = spawn(async move {
        while !stop_ticker_for_task.load(Ordering::Relaxed) {
            sleep(Duration::from_millis(50)).await;
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

#[tokio_test]
async fn test_retry_eligibility_uses_intercepted_method() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 503,
        headers: vec![],
        body: b"unavailable".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |request: &mut HttpRequest| {
        observed.fetch_add(1, Ordering::SeqCst);
        request.set_method(Method::POST);
        Ok(())
    }));
    let error = client
        .execute(client.request(Method::GET, "/intercepted-post").build())
        .await
        .expect_err("intercepted POST is not replayable by the default policy");
    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.method, Some(Method::POST));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(server.finish().await.method, "POST");
}

#[tokio_test]
async fn test_intercepted_post_retries_when_final_override_allows_it() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 503,
            headers: vec![],
            body: b"retry".to_vec(),
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
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |request: &mut HttpRequest| {
        observed.fetch_add(1, Ordering::SeqCst);
        request.set_method(Method::POST);
        request.set_retry_override(
            qubit_http::HttpRequestRetryOverride::new().with_method_policy(HttpRetryMethodPolicy::AllMethods),
        );
        Ok(())
    }));
    let response = client
        .execute(client.request(Method::GET, "/intercepted-allow-post").build())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let requests = server.finish().await;
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request.method == "POST"));
}

#[tokio_test]
async fn test_original_post_is_not_admitted_after_interceptor_changes_to_get() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 503,
        headers: vec![],
        body: b"unavailable".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |request: &mut HttpRequest| {
        observed.fetch_add(1, Ordering::SeqCst);
        request.set_method(Method::GET);
        Ok(())
    }));
    let error = client
        .execute(client.request(Method::POST, "/post-to-get").build())
        .await
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(server.finish().await.method, "GET");
}

#[tokio_test]
async fn test_interceptor_transport_failure_uses_final_method_and_preserves_retry_source() {
    let mut options = HttpClientOptions::default();
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |request: &mut HttpRequest| {
        observed.fetch_add(1, Ordering::SeqCst);
        request.set_method(Method::POST);
        Err(HttpError::transport("interceptor transport failure"))
    }));
    let error = client
        .execute(client.request(Method::GET, "http://127.0.0.1:9/final-method").build())
        .await
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Transport);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    let retry = error
        .source
        .as_deref()
        .and_then(|source| (source as &(dyn StdError + 'static)).downcast_ref::<RetryError<HttpError>>())
        .expect("public source retains RetryError<HttpError>");
    assert_eq!(retry.context().attempts(), 1);
}

#[tokio_test]
async fn test_interceptor_force_disable_stops_an_admitted_retry_flow() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 503,
        headers: vec![],
        body: b"unavailable".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.backoff = BackoffPolicy::immediate();
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    client.add_request_interceptor(HttpRequestInterceptor::new(move |request: &mut HttpRequest| {
        observed.fetch_add(1, Ordering::SeqCst);
        request.set_retry_override(HttpRequestRetryOverride::new().force_disable());
        Ok(())
    }));
    let error = client
        .execute(client.request(Method::GET, "/interceptor-disable").build())
        .await
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    let _ = server.finish().await;
}
