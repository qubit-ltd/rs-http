/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use qubit_http::{
    HttpClientFactory, HttpClientOptions, HttpError, HttpErrorKind, HttpHeaderInjector,
    HttpRequestInterceptor, HttpResponseInterceptor,
};
use tokio::time::timeout;

use crate::common::{spawn_one_shot_server, ResponsePlan};

#[test]
fn test_http_client_debug_includes_options_and_injectors() {
    let mut client = HttpClientFactory::new()
        .create_default()
        .expect("default options should create client");
    client.add_header_injector(HttpHeaderInjector::new(|_headers: &mut HeaderMap| Ok(())));

    let output = format!("{:?}", client);

    assert!(output.contains("HttpClient"));
    assert!(output.contains("options"));
    assert!(output.contains("injectors"));
}

#[tokio::test]
async fn test_absolute_url_request_bypasses_base_url_join() {
    let target_server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    // Deliberately points to a non-existing host; absolute URL should bypass this.
    options.base_url = Some(url::Url::parse("http://127.0.0.1:1/").unwrap());
    let client = HttpClientFactory::new()
        .create(options)
        .unwrap();

    let path = format!("{}absolute", target_server.base_url());
    let request = client.request(Method::GET, path.as_str()).build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.meta.status.as_u16(), 200);

    let captured = timeout(Duration::from_secs(3), target_server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/absolute");
}

#[tokio::test]
async fn test_execute_returns_invalid_url_for_bad_relative_path() {
    let mut options = HttpClientOptions::default();
    options.base_url = Some(
        url::Url::parse("https://example.com/api/").expect("static base_url in test should parse"),
    );
    let client = HttpClientFactory::new()
        .create(options)
        .expect("valid options should create client");
    let request = client.request(Method::GET, "http://[::1").build();

    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("invalid relative path should fail before request is sent");

    assert_eq!(error.kind, HttpErrorKind::InvalidUrl);
    assert!(error
        .message
        .contains("Failed to resolve path 'http://[::1'"));
}

#[tokio::test]
async fn test_header_injector_order_is_stable_and_clear_works() {
    let server1 = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server1.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .unwrap();
    client.add_header_injector(HttpHeaderInjector::new(|headers: &mut HeaderMap| {
        headers.insert(
            HeaderName::from_static("x-seq"),
            HeaderValue::from_static("A"),
        );
        Ok(())
    }));
    client.add_header_injector(HttpHeaderInjector::new(|headers: &mut HeaderMap| {
        headers.insert(
            HeaderName::from_static("x-seq"),
            HeaderValue::from_static("B"),
        );
        Ok(())
    }));

    let request = client.request(Method::GET, "/ordered").build();
    let _ = client.execute(request).await.unwrap();
    let captured = server1.finish().await;
    assert_eq!(captured.headers.get("x-seq"), Some(&"B".to_string()));

    let server2 = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options2 = HttpClientOptions::default();
    options2.base_url = Some(server2.base_url());
    let mut client2 = HttpClientFactory::new()
        .create(options2)
        .unwrap();
    client2.add_header_injector(HttpHeaderInjector::new(|headers: &mut HeaderMap| {
        headers.insert(
            HeaderName::from_static("x-seq"),
            HeaderValue::from_static("A"),
        );
        Ok(())
    }));
    client2.clear_header_injectors();
    let request2 = client2.request(Method::GET, "/cleared").build();
    let _ = client2.execute(request2).await.unwrap();
    let captured2 = server2.finish().await;
    assert!(!captured2.headers.contains_key("x-seq"));
}

#[tokio::test]
async fn test_failing_header_injector_short_circuits_request() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .unwrap();
    client.add_header_injector(HttpHeaderInjector::new(|_headers: &mut HeaderMap| {
        Err(HttpError::other("inject failed"))
    }));

    let request = client.request(Method::GET, "/will-not-send").build();
    let error = client.execute(request).await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("inject failed"));
}

#[tokio::test]
async fn test_request_interceptor_order_is_stable_and_clear_works() {
    let server1 = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server1.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .unwrap();
    client.add_request_interceptor(HttpRequestInterceptor::new(|request| {
        request.set_typed_header(
            HeaderName::from_static("x-request-seq"),
            HeaderValue::from_static("A"),
        );
        request.add_query_param("request_interceptor", "first");
        Ok(())
    }));
    client.add_request_interceptor(HttpRequestInterceptor::new(|request| {
        request.set_typed_header(
            HeaderName::from_static("x-request-seq"),
            HeaderValue::from_static("B"),
        );
        request.add_query_param("request_interceptor", "second");
        Ok(())
    }));

    let request = client
        .request(Method::GET, "/request-order")
        .query_param("initial", "1")
        .build();
    let _ = client.execute(request).await.unwrap();
    let captured = server1.finish().await;
    assert_eq!(
        captured.headers.get("x-request-seq"),
        Some(&"B".to_string())
    );
    assert!(captured.target.contains("initial=1"));
    assert!(captured.target.contains("request_interceptor=first"));
    assert!(captured.target.contains("request_interceptor=second"));

    let server2 = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options2 = HttpClientOptions::default();
    options2.base_url = Some(server2.base_url());
    let mut client2 = HttpClientFactory::new()
        .create(options2)
        .unwrap();
    client2.add_request_interceptor(HttpRequestInterceptor::new(|request| {
        request.set_typed_header(
            HeaderName::from_static("x-request-cleared"),
            HeaderValue::from_static("yes"),
        );
        Ok(())
    }));
    client2.clear_request_interceptors();

    let request2 = client2.request(Method::GET, "/request-clear").build();
    let _ = client2.execute(request2).await.unwrap();
    let captured2 = server2.finish().await;
    assert!(!captured2.headers.contains_key("x-request-cleared"));
}

#[tokio::test]
async fn test_failing_request_interceptor_short_circuits_before_url_resolution() {
    let mut client = HttpClientFactory::new()
        .create_default()
        .expect("default options should create client");
    client.add_request_interceptor(HttpRequestInterceptor::new(|_request| {
        Err(HttpError::other("request blocked by interceptor"))
    }));

    let request = client
        .request(
            Method::GET,
            "http://127.0.0.1:1/request-interceptor-blocked",
        )
        .build();
    let error = client.execute(request).await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("request blocked by interceptor"));
    assert_eq!(error.method, Some(Method::GET));
    let error_url = error
        .url
        .expect("request interceptor error should include URL");
    assert_eq!(
        error_url.as_str(),
        "http://127.0.0.1:1/request-interceptor-blocked"
    );
}

#[tokio::test]
async fn test_response_interceptor_order_is_stable_and_short_circuits() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .unwrap();

    let events = Arc::new(Mutex::new(Vec::new()));
    let first_events = Arc::clone(&events);
    client.add_response_interceptor(HttpResponseInterceptor::new(move |_meta| {
        first_events
            .lock()
            .expect("lock response interceptor events for first")
            .push("first".to_string());
        Ok(())
    }));
    let second_events = Arc::clone(&events);
    client.add_response_interceptor(HttpResponseInterceptor::new(move |_meta| {
        second_events
            .lock()
            .expect("lock response interceptor events for second")
            .push("second".to_string());
        Err(HttpError::other("response blocked by interceptor"))
    }));

    let request = client.request(Method::GET, "/response-order").build();
    let error = client.execute(request).await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert_eq!(error.status.map(|status| status.as_u16()), Some(200));
    assert_eq!(error.method, Some(Method::GET));
    assert!(error.url.is_some());
    assert!(error.message.contains("response blocked by interceptor"));
    assert_eq!(
        *events
            .lock()
            .expect("lock response interceptor events for assertion"),
        vec!["first".to_string(), "second".to_string()]
    );
}

#[tokio::test]
async fn test_clear_response_interceptors_restores_success_path() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .unwrap();
    client.add_response_interceptor(HttpResponseInterceptor::new(|_meta| {
        Err(HttpError::other("should be cleared"))
    }));
    client.clear_response_interceptors();

    let request = client.request(Method::GET, "/response-clear").build();
    let response = client.execute(request).await.unwrap();
    assert_eq!(response.meta.status.as_u16(), 200);
}

#[tokio::test]
async fn test_execute_applies_response_interceptor_for_unconsumed_body() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![],
        chunks: vec![],
        finish: true,
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .unwrap();
    let called = Arc::new(AtomicUsize::new(0));
    let called_for_interceptor = Arc::clone(&called);
    client.add_response_interceptor(HttpResponseInterceptor::new(move |_meta| {
        called_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }));

    let request = client
        .request(Method::GET, "/stream-response-interceptor")
        .build();
    let response = client.execute(request).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(called.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn test_request_url_can_differ_from_response_meta_url() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("valid options should create client");
    let rewritten_url = url::Url::parse("https://interceptor.example/rewritten")
        .expect("static interceptor URL should parse");
    let rewritten_url_for_interceptor = rewritten_url.clone();
    client.add_response_interceptor(HttpResponseInterceptor::new(move |meta| {
        meta.url = rewritten_url_for_interceptor.clone();
        Ok(())
    }));

    let request = client.request(Method::GET, "/request-url-diff").build();
    let response = client
        .execute(request)
        .await
        .expect("request should succeed");
    let expected_request_url = server
        .base_url()
        .join("request-url-diff")
        .expect("request URL should join");

    assert_eq!(response.request_url(), &expected_request_url);
    assert_eq!(response.url(), &rewritten_url);
    assert_ne!(response.request_url(), response.url());

    let captured = server.finish().await;
    assert_eq!(captured.target, "/request-url-diff");
}

#[tokio::test]
async fn test_request_url_is_used_in_buffered_read_error() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![],
        chunks: vec![
            crate::common::ResponseChunk {
                delay: Duration::from_millis(0),
                bytes: b"partial".to_vec(),
            },
            crate::common::ResponseChunk {
                delay: Duration::from_millis(200),
                bytes: b"later".to_vec(),
            },
        ],
        finish: true,
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.timeouts.read_timeout = Duration::from_millis(50);
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("valid options should create client");
    let interceptor_url = url::Url::parse("https://interceptor.example/context-url-rewritten")
        .expect("static interceptor URL should parse");
    let expected_request_url = server
        .base_url()
        .join("context-url-timeout")
        .expect("request URL should join");
    let expected_url_for_interceptor = interceptor_url.clone();
    client.add_response_interceptor(HttpResponseInterceptor::new(move |meta| {
        meta.url = expected_url_for_interceptor.clone();
        Ok(())
    }));

    let request = client.request(Method::GET, "/context-url-timeout").build();
    let mut response = client.execute(request).await.expect("request should start");
    let error = response
        .bytes()
        .await
        .expect_err("buffered read should timeout");

    assert_eq!(error.kind, HttpErrorKind::ReadTimeout);
    assert_eq!(error.url, Some(expected_request_url));
    let captured = server.finish().await;
    assert_eq!(captured.target, "/context-url-timeout");
}

#[tokio::test]
async fn test_retry_status_code_allowlist_can_disable_retry_for_503() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
        headers: vec![],
        body: b"retry later".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.logging.enabled = false;
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.retry_status_codes = Some(vec![StatusCode::TOO_MANY_REQUESTS]);
    let mut client = HttpClientFactory::new()
        .create(options)
        .unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request| {
        attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }));

    let request = client.request(Method::GET, "/retry-status-filter").build();
    let error = client.execute(request).await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.status, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn test_retry_status_code_allowlist_can_enable_retry_for_503() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
        headers: vec![],
        body: b"retry later".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.logging.enabled = false;
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.retry_status_codes = Some(vec![StatusCode::SERVICE_UNAVAILABLE]);
    options.retry.retry_error_kinds = Some(vec![HttpErrorKind::Status]);
    let mut client = HttpClientFactory::new()
        .create(options)
        .unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request| {
        attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }));

    let request = client.request(Method::GET, "/retry-status-allow").build();
    let error = client.execute(request).await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Transport);
    assert_eq!(attempts.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn test_retry_error_kind_allowlist_can_disable_transport_retry() {
    let mut options = HttpClientOptions::default();
    options.logging.enabled = false;
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.retry_error_kinds = Some(vec![HttpErrorKind::ReadTimeout]);
    let mut client = HttpClientFactory::new()
        .create(options)
        .unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request| {
        attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }));

    let request = client
        .request(Method::GET, "http://127.0.0.1:9/retry-kind-filter")
        .build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Transport);
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn test_add_header_applies_client_default_header() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .unwrap();
    client.add_header("x-client", "default").unwrap();

    let request = client.request(Method::GET, "/default-header").build();
    let _ = client.execute(request).await.unwrap();
    let captured = server.finish().await;
    assert_eq!(
        captured.headers.get("x-client"),
        Some(&"default".to_string())
    );
}

#[tokio::test]
async fn test_add_headers_is_atomic_and_request_header_still_overrides() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .unwrap();
    client
        .add_headers([
            ("x-batch-a", "value-a"),
            ("x-order", "client"),
            ("x-batch-b", "value-b"),
        ])
        .unwrap();

    let request = client
        .request(Method::GET, "/batch-headers")
        .header("x-order", "request")
        .unwrap()
        .build();
    let _ = client.execute(request).await.unwrap();
    let captured = server.finish().await;
    assert_eq!(
        captured.headers.get("x-batch-a"),
        Some(&"value-a".to_string())
    );
    assert_eq!(
        captured.headers.get("x-batch-b"),
        Some(&"value-b".to_string())
    );
    assert_eq!(
        captured.headers.get("x-order"),
        Some(&"request".to_string())
    );
}

#[tokio::test]
async fn test_add_headers_invalid_batch_does_not_partially_apply() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .unwrap();

    let error = client
        .add_headers([("x-valid", "kept-out"), ("bad header", "boom")])
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("Invalid header name"));

    let request = client.request(Method::GET, "/atomic").build();
    let _ = client.execute(request).await.unwrap();
    let captured = server.finish().await;
    assert!(!captured.headers.contains_key("x-valid"));
}

#[tokio::test]
async fn test_add_header_invalid_value_does_not_apply() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .unwrap();

    let error = client.add_header("x-bad", "line1\nline2").unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("Invalid header value"));

    let request = client.request(Method::GET, "/invalid-value").build();
    let _ = client.execute(request).await.unwrap();
    let captured = server.finish().await;
    assert!(!captured.headers.contains_key("x-bad"));
}

#[tokio::test]
async fn test_add_header_injector_still_overrides_client_default_header() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .unwrap();
    client.add_header("x-order", "client").unwrap();
    client.add_header_injector(HttpHeaderInjector::new(|headers: &mut HeaderMap| {
        headers.insert(
            HeaderName::from_static("x-order"),
            HeaderValue::from_static("injector"),
        );
        Ok(())
    }));

    let request = client.request(Method::GET, "/injector-overrides").build();
    let _ = client.execute(request).await.unwrap();
    let captured = server.finish().await;
    assert_eq!(
        captured.headers.get("x-order"),
        Some(&"injector".to_string())
    );
}

#[tokio::test]
async fn test_clone_default_headers_are_independent_after_creation() {
    let server_original = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let server_clone = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut client = HttpClientFactory::new().create_default().unwrap();
    client.add_header("x-shared", "base").unwrap();

    let mut cloned = client.clone();
    cloned.add_header("x-clone-only", "yes").unwrap();
    client.add_header("x-origin-only", "yes").unwrap();

    let path = format!("{}origin", server_original.base_url());
    let request_original = client.request(Method::GET, path.as_str()).build();
    let path = format!("{}clone", server_clone.base_url());
    let request_clone = cloned.request(Method::GET, path.as_str()).build();

    let _ = client.execute(request_original).await.unwrap();
    let _ = cloned.execute(request_clone).await.unwrap();

    let captured_original = server_original.finish().await;
    let captured_clone = server_clone.finish().await;

    assert_eq!(
        captured_original.headers.get("x-shared"),
        Some(&"base".to_string())
    );
    assert_eq!(
        captured_clone.headers.get("x-shared"),
        Some(&"base".to_string())
    );
    assert_eq!(
        captured_original.headers.get("x-origin-only"),
        Some(&"yes".to_string())
    );
    assert!(!captured_original.headers.contains_key("x-clone-only"));
    assert_eq!(
        captured_clone.headers.get("x-clone-only"),
        Some(&"yes".to_string())
    );
    assert!(!captured_clone.headers.contains_key("x-origin-only"));
}
