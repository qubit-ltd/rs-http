// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error as StdError;
use std::io::ErrorKind;
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use http::HeaderMap;
use http::HeaderName;
use http::HeaderValue;
use http::Method;
use http::StatusCode;
use qubit_http::AsyncHttpHeaderInjector;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpError;
use qubit_http::HttpErrorKind;
use qubit_http::HttpHeaderInjector;
use qubit_http::HttpOriginPolicy;
use qubit_http::HttpRequest;
use qubit_http::HttpRequestInterceptor;
use qubit_http::HttpResponseInterceptor;
use qubit_http::HttpResponseInterceptorContext;
use qubit_retry::BackoffPolicy;
use qubit_retry::RetryError;
use tokio::test as tokio_test;
use tokio::time::timeout;

use crate::common::ResponsePlan;
use crate::common::spawn_multi_shot_server;
use crate::common::spawn_one_shot_server;

fn retry_abort_inner_http(error: &HttpError) -> &HttpError {
    let boxed = error.source.as_ref().expect("retry abort should chain inner error");
    (boxed.as_ref() as &(dyn StdError + 'static))
        .downcast_ref::<RetryError<HttpError>>()
        .expect("source should retain the complete retry error")
        .last_error()
        .expect("retry error should retain the HTTP attempt")
}

#[test]
fn test_http_client_debug_includes_options_and_injectors() {
    let mut client = HttpClientBuilder::new()
        .build()
        .expect("default options should create client");
    client.add_header_injector(HttpHeaderInjector::new(|_headers: &mut HeaderMap| Ok(())));

    let output = format!("{:?}", client);

    assert!(output.contains("HttpClient"));
    assert!(output.contains("options"));
    assert!(output.contains("injectors"));
}

#[tokio_test]
async fn test_rebuild_with_options_preserves_registered_hooks() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut client = HttpClientBuilder::new()
        .build()
        .expect("default client should be created");
    client.add_header_injector(HttpHeaderInjector::new(|headers: &mut HeaderMap| {
        headers.insert(HeaderName::from_static("x-sync"), HeaderValue::from_static("yes"));
        Ok(())
    }));
    client.add_async_header_injector(AsyncHttpHeaderInjector::new(|headers: &mut HeaderMap| {
        Box::pin(async move {
            headers.insert(HeaderName::from_static("x-async"), HeaderValue::from_static("yes"));
            Ok(())
        })
    }));
    client.add_request_interceptor(HttpRequestInterceptor::new(|request: &mut HttpRequest| {
        request.set_header("x-request", "yes")?;
        Ok(())
    }));
    client.add_response_interceptor(HttpResponseInterceptor::new(
        |context: &mut HttpResponseInterceptorContext| {
            context
                .headers_mut()
                .insert(HeaderName::from_static("x-response"), HeaderValue::from_static("yes"));
            Ok(())
        },
    ));

    let mut options = client.options().clone();
    options.base_url = Some(server.base_url());
    let rebuilt = client
        .rebuild_with_options(options)
        .expect("valid options should rebuild the client");
    assert!(
        client.options().base_url.is_none(),
        "original client should remain unchanged"
    );

    let request = rebuilt.request(Method::GET, "/rebuilt").build();
    let response = rebuilt
        .execute(request)
        .await
        .expect("rebuilt client should send request");
    assert_eq!(
        response.headers().get("x-response"),
        Some(&HeaderValue::from_static("yes"))
    );
    let captured = server.finish().await;
    assert_eq!(captured.headers.get("x-sync").map(String::as_str), Some("yes"));
    assert_eq!(captured.headers.get("x-async").map(String::as_str), Some("yes"));
    assert_eq!(captured.headers.get("x-request").map(String::as_str), Some("yes"));
}

#[test]
fn test_rebuild_with_options_rejects_invalid_options_without_changing_client() {
    let client = HttpClientBuilder::new()
        .build()
        .expect("default client should be created");
    let mut invalid = client.options().clone();
    invalid.timeouts.connect_timeout = Duration::ZERO;

    let result = client.rebuild_with_options(invalid);

    assert!(result.is_err(), "invalid options must not create a new client");
    assert!(client.options().timeouts.connect_timeout > Duration::ZERO);
}

#[tokio_test]
async fn test_absolute_url_request_bypasses_base_url_join() {
    let target_server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let mut options = HttpClientOptions::default();
    // Deliberately points to a non-existing host; absolute URL should bypass
    // this.
    options.base_url = Some(url::Url::parse("http://127.0.0.1:1/").unwrap());
    options.origin_policy = HttpOriginPolicy::AnyOrigin;
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let path = format!("{}absolute", target_server.base_url());
    let request = client.request(Method::GET, path.as_str()).build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);

    let captured = timeout(Duration::from_secs(3), target_server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.target, "/absolute");
}

#[tokio_test]
async fn test_default_client_executes_absolute_url_without_base_url() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let client = HttpClientBuilder::new().build().unwrap();
    let request = client
        .request(Method::GET, &format!("{}absolute-default", server.base_url()))
        .build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(server.finish().await.target, "/absolute-default");
}

#[tokio_test]
async fn test_default_client_rejects_relative_path_without_base_url() {
    let client = HttpClientBuilder::new().build().unwrap();
    let error = client
        .execute(client.request(Method::GET, "/relative").build())
        .await
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::InvalidUrl);
}

#[tokio_test]
async fn test_same_origin_rejects_absolute_url_different_from_configured_base() {
    let mut options = HttpClientOptions::default();
    options.base_url = Some(url::Url::parse("http://127.0.0.1:1/").unwrap());
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let error = client
        .execute(client.request(Method::GET, "http://127.0.0.1:2/cross-origin").build())
        .await
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::OriginPolicy);
}

#[tokio_test]
async fn test_same_origin_redirect_reaches_same_origin_target() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 302,
            headers: vec![
                ("Location".to_string(), "/final".to_string()),
                ("Connection".to_string(), "close".to_string()),
            ],
            body: vec![],
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
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let response = timeout(
        Duration::from_secs(3),
        client.execute(client.request(Method::GET, "/start").build()),
    )
    .await
    .expect("redirect request timed out")
    .expect("same-origin redirect should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("redirect server did not finish");
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].target, "/start");
    assert_eq!(captured[1].target, "/final");
}

#[tokio_test]
async fn test_same_origin_redirect_rejects_cross_origin_before_sending_credentials() {
    let target = TcpListener::bind("127.0.0.1:0").expect("cross-origin target should bind");
    target
        .set_nonblocking(true)
        .expect("cross-origin target should be nonblocking");
    let target_url = format!(
        "http://{}/stolen",
        target.local_addr().expect("target address should exist")
    );
    let source = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 302,
        headers: vec![("Location".to_string(), target_url)],
        body: vec![],
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(source.base_url());
    options
        .add_header("authorization", "Bearer sensitive-value")
        .expect("authorization header should be valid");
    options
        .add_header("x-api-key", "api-key-value")
        .expect("API key header should be valid");
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let error = timeout(
        Duration::from_secs(3),
        client.execute(client.request(Method::GET, "/redirect").build()),
    )
    .await
    .expect("cross-origin redirect request timed out")
    .expect_err("cross-origin redirect should fail");

    assert_eq!(error.kind, HttpErrorKind::OriginPolicy);
    let captured = source.finish().await;
    assert_eq!(
        captured.headers.get("authorization").map(String::as_str),
        Some("Bearer sensitive-value")
    );
    assert_eq!(
        captured.headers.get("x-api-key").map(String::as_str),
        Some("api-key-value")
    );
    assert_eq!(
        target
            .accept()
            .expect_err("cross-origin target must not receive a connection")
            .kind(),
        ErrorKind::WouldBlock
    );
}

#[tokio_test]
async fn test_any_origin_redirect_forwards_custom_header_but_strips_authorization() {
    let target = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let target_url = format!("{}target", target.base_url());
    let source = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 302,
        headers: vec![("Location".to_string(), target_url)],
        body: vec![],
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(source.base_url());
    options.origin_policy = HttpOriginPolicy::AnyOrigin;
    options.add_header("x-api-key", "api-key-value").unwrap();
    options
        .add_header("authorization", "Bearer authorization-value")
        .unwrap();
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    let response = timeout(
        Duration::from_secs(3),
        client.execute(client.request(Method::GET, "/redirect").build()),
    )
    .await
    .expect("AnyOrigin redirect request timed out")
    .expect("AnyOrigin redirect should succeed");
    assert_eq!(response.status(), StatusCode::OK);
    let source_request = timeout(Duration::from_secs(3), source.finish())
        .await
        .expect("redirect source server did not finish");
    let target_request = timeout(Duration::from_secs(3), target.finish())
        .await
        .expect("redirect target server did not finish");
    assert_eq!(
        source_request.headers.get("x-api-key").map(String::as_str),
        Some("api-key-value")
    );
    assert_eq!(target_request.target, "/target");
    assert_eq!(
        target_request.headers.get("x-api-key").map(String::as_str),
        Some("api-key-value")
    );
    assert!(!target_request.headers.contains_key("authorization"));
}

#[tokio_test]
async fn test_any_origin_redirect_forwards_sensitive_custom_header() {
    let target = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let target_url = format!("{}target", target.base_url());
    let source = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 302,
        headers: vec![("Location".to_string(), target_url)],
        body: vec![],
    })
    .await;
    let mut api_key = HeaderValue::from_static("sensitive-api-key-value");
    api_key.set_sensitive(true);
    let mut options = HttpClientOptions::default();
    options.base_url = Some(source.base_url());
    options.origin_policy = HttpOriginPolicy::AnyOrigin;
    options
        .default_headers
        .insert(HeaderName::from_static("x-api-key"), api_key);
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    timeout(
        Duration::from_secs(3),
        client.execute(client.request(Method::GET, "/redirect").build()),
    )
    .await
    .expect("sensitive-header redirect request timed out")
    .expect("AnyOrigin redirect with a sensitive header should succeed");
    let _ = timeout(Duration::from_secs(3), source.finish())
        .await
        .expect("redirect source server did not finish");
    let target_request = timeout(Duration::from_secs(3), target.finish())
        .await
        .expect("redirect target server did not finish");
    assert_eq!(
        target_request.headers.get("x-api-key").map(String::as_str),
        Some("sensitive-api-key-value")
    );
}

#[tokio_test]
async fn test_same_origin_redirect_preserves_custom_header() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 302,
            headers: vec![
                ("Location".to_string(), "/final".to_string()),
                ("Connection".to_string(), "close".to_string()),
            ],
            body: vec![],
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
    options.add_header("x-api-key", "same-origin-api-key").unwrap();
    let client = HttpClientBuilder::new().options(options).build().unwrap();

    timeout(
        Duration::from_secs(3),
        client.execute(client.request(Method::GET, "/start").build()),
    )
    .await
    .expect("same-origin custom-header redirect timed out")
    .expect("same-origin redirect should succeed");
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("same-origin redirect server did not finish");
    assert_eq!(captured.len(), 2);
    assert!(
        captured
            .iter()
            .all(|request| { request.headers.get("x-api-key").map(String::as_str) == Some("same-origin-api-key") })
    );
}

#[tokio_test]
async fn test_same_origin_redirect_honors_max_redirects() {
    let server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 302,
            headers: vec![
                ("Location".to_string(), "/again".to_string()),
                ("Connection".to_string(), "close".to_string()),
            ],
            body: vec![],
        },
        ResponsePlan::Immediate {
            status: 302,
            headers: vec![("Location".to_string(), "/never".to_string())],
            body: vec![],
        },
    ])
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.max_redirects = Some(1);
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");

    let result = timeout(
        Duration::from_secs(3),
        client.execute(client.request(Method::GET, "/start").build()),
    )
    .await
    .expect("redirect-limited request timed out");

    assert!(result.is_err(), "redirect beyond the configured limit should fail");
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("redirect server did not finish");
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[1].target, "/again");
}

#[tokio_test]
async fn test_execute_returns_invalid_url_for_bad_relative_path() {
    let mut options = HttpClientOptions::default();
    options.base_url = Some(url::Url::parse("https://example.com/api/").expect("static base_url in test should parse"));
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("valid options should create client");
    let request = client.request(Method::GET, "http://[::1").build();

    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("invalid relative path should fail before request is sent");

    assert_eq!(error.kind, HttpErrorKind::InvalidUrl);
    assert!(
        error
            .message
            .contains("Failed to resolve request path against base URL")
    );
}

#[tokio_test]
async fn test_header_injector_order_is_stable_and_clear_works() {
    let server1 = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server1.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client.add_header_injector(HttpHeaderInjector::new(|headers: &mut HeaderMap| {
        headers.insert(HeaderName::from_static("x-seq"), HeaderValue::from_static("A"));
        Ok(())
    }));
    client.add_header_injector(HttpHeaderInjector::new(|headers: &mut HeaderMap| {
        headers.insert(HeaderName::from_static("x-seq"), HeaderValue::from_static("B"));
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
    let mut client2 = HttpClientBuilder::new().options(options2).build().unwrap();
    client2.add_header_injector(HttpHeaderInjector::new(|headers: &mut HeaderMap| {
        headers.insert(HeaderName::from_static("x-seq"), HeaderValue::from_static("A"));
        Ok(())
    }));
    client2.clear_header_injectors();
    let request2 = client2.request(Method::GET, "/cleared").build();
    let _ = client2.execute(request2).await.unwrap();
    let captured2 = server2.finish().await;
    assert!(!captured2.headers.contains_key("x-seq"));
}

#[tokio_test]
async fn test_failing_header_injector_short_circuits_request() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client.add_header_injector(HttpHeaderInjector::new(|_headers: &mut HeaderMap| {
        Err(HttpError::other("inject failed"))
    }));

    let request = client.request(Method::GET, "/will-not-send").build();
    let error = client.execute(request).await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("inject failed"));
}

#[tokio_test]
async fn test_request_interceptor_order_is_stable_and_clear_works() {
    let server1 = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server1.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client.add_request_interceptor(HttpRequestInterceptor::new(|request: &mut HttpRequest| {
        request.set_typed_header(HeaderName::from_static("x-request-seq"), HeaderValue::from_static("A"));
        request.add_query_param("request_interceptor", "first");
        Ok(())
    }));
    client.add_request_interceptor(HttpRequestInterceptor::new(|request: &mut HttpRequest| {
        request.set_typed_header(HeaderName::from_static("x-request-seq"), HeaderValue::from_static("B"));
        request.add_query_param("request_interceptor", "second");
        Ok(())
    }));

    let request = client
        .request(Method::GET, "/request-order")
        .query_param("initial", "1")
        .build();
    let _ = client.execute(request).await.unwrap();
    let captured = server1.finish().await;
    assert_eq!(captured.headers.get("x-request-seq"), Some(&"B".to_string()));
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
    let mut client2 = HttpClientBuilder::new().options(options2).build().unwrap();
    client2.add_request_interceptor(HttpRequestInterceptor::new(|request: &mut HttpRequest| {
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

#[tokio_test]
async fn test_failing_request_interceptor_short_circuits_before_url_resolution() {
    let mut client = HttpClientBuilder::new()
        .build()
        .expect("default options should create client");
    client.add_request_interceptor(HttpRequestInterceptor::new(|_request: &mut HttpRequest| {
        Err(HttpError::other("request blocked by interceptor"))
    }));

    let request = client
        .request(Method::GET, "http://127.0.0.1:1/request-interceptor-blocked")
        .build();
    let error = client.execute(request).await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("request blocked by interceptor"));
    assert_eq!(error.method, Some(Method::GET));
    let error_url = error.url.expect("request interceptor error should include URL");
    assert_eq!(error_url.as_str(), "http://127.0.0.1:1/request-interceptor-blocked");
}

#[tokio_test]
async fn test_response_interceptor_order_is_stable_and_short_circuits() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();

    let events = Arc::new(Mutex::new(Vec::new()));
    let first_events = Arc::clone(&events);
    client.add_response_interceptor(HttpResponseInterceptor::new(
        move |_meta: &mut HttpResponseInterceptorContext| {
            first_events
                .lock()
                .expect("lock response interceptor events for first")
                .push("first".to_string());
            Ok(())
        },
    ));
    let second_events = Arc::clone(&events);
    client.add_response_interceptor(HttpResponseInterceptor::new(
        move |_meta: &mut HttpResponseInterceptorContext| {
            second_events
                .lock()
                .expect("lock response interceptor events for second")
                .push("second".to_string());
            Err(HttpError::other("response blocked by interceptor"))
        },
    ));

    let request = client.request(Method::GET, "/response-order").build();
    let error = client.execute(request).await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert_eq!(error.status.map(|status| status.as_u16()), Some(200));
    assert_eq!(error.method, Some(Method::GET));
    assert!(error.url.is_some());
    assert!(error.message.contains("response blocked by interceptor"));
    assert_eq!(
        *events.lock().expect("lock response interceptor events for assertion"),
        vec!["first".to_string(), "second".to_string()]
    );
}

#[tokio_test]
async fn test_clear_response_interceptors_restores_success_path() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client.add_response_interceptor(HttpResponseInterceptor::new(
        |_meta: &mut HttpResponseInterceptorContext| Err(HttpError::other("should be cleared")),
    ));
    client.clear_response_interceptors();

    let request = client.request(Method::GET, "/response-clear").build();
    let response = client.execute(request).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
}

#[tokio_test]
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
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let called = Arc::new(AtomicUsize::new(0));
    let called_for_interceptor = Arc::clone(&called);
    client.add_response_interceptor(HttpResponseInterceptor::new(
        move |_meta: &mut HttpResponseInterceptorContext| {
            called_for_interceptor.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
    ));

    let request = client.request(Method::GET, "/stream-response-interceptor").build();
    let response = client.execute(request).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(called.load(Ordering::Relaxed), 1);
}

#[tokio_test]
async fn test_original_content_length_header_increase_does_not_reject_small_body() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![("Content-Length".to_string(), "2".to_string())],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.response_body_size_limit = 4;
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client.add_response_interceptor(HttpResponseInterceptor::new(
        |context: &mut HttpResponseInterceptorContext| {
            context.headers_mut().insert(
                HeaderName::from_static("content-length"),
                HeaderValue::from_static("999"),
            );
            Ok(())
        },
    ));

    let mut response = client
        .execute(client.request(Method::GET, "/original-content-length-increase").build())
        .await
        .unwrap();
    assert_eq!(response.headers().get("content-length").unwrap(), "999");
    assert_eq!(response.bytes().await.unwrap().as_ref(), b"ok");
    server.finish().await;
}

#[tokio_test]
async fn test_original_content_length_still_rejects_when_header_is_reduced() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![("Content-Length".to_string(), "9".to_string())],
        body: b"12345".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.response_body_size_limit = 4;
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client.add_response_interceptor(HttpResponseInterceptor::new(
        |context: &mut HttpResponseInterceptorContext| {
            context
                .headers_mut()
                .insert(HeaderName::from_static("content-length"), HeaderValue::from_static("1"));
            Ok(())
        },
    ));

    let mut response = client
        .execute(client.request(Method::GET, "/original-content-length-reduced").build())
        .await
        .unwrap();
    let first_error = response.bytes().await.unwrap_err();
    assert_eq!(first_error.kind, HttpErrorKind::ResponseBodyTooLarge);
    assert!(first_error.message.contains("observed 9 bytes"));
    let repeated_error = response.bytes().await.unwrap_err();
    assert_eq!(repeated_error.kind, first_error.kind);
    server.finish().await;
}

#[tokio_test]
async fn test_original_content_length_still_rejects_when_header_is_removed() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![("Content-Length".to_string(), "9".to_string())],
        body: b"12345".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.response_body_size_limit = 4;
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client.add_response_interceptor(HttpResponseInterceptor::new(
        |context: &mut HttpResponseInterceptorContext| {
            context.headers_mut().remove("content-length");
            Ok(())
        },
    ));

    let mut response = client
        .execute(client.request(Method::GET, "/original-content-length-removed").build())
        .await
        .unwrap();
    let first_error = response.bytes().await.unwrap_err();
    assert_eq!(first_error.kind, HttpErrorKind::ResponseBodyTooLarge);
    assert!(first_error.message.contains("observed 9 bytes"));
    let repeated_error = response.bytes().await.unwrap_err();
    assert_eq!(repeated_error.kind, first_error.kind);
    server.finish().await;
}

#[tokio_test]
async fn test_original_content_length_chunked_without_length_still_enforces_limit() {
    let server = spawn_one_shot_server(ResponsePlan::Chunked {
        status: 200,
        headers: vec![],
        chunks: vec![crate::common::ResponseChunk {
            delay: Duration::ZERO,
            bytes: b"oversized".to_vec(),
        }],
        finish: true,
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.response_body_size_limit = 4;
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client.add_response_interceptor(HttpResponseInterceptor::new(
        |context: &mut HttpResponseInterceptorContext| {
            context
                .headers_mut()
                .insert(HeaderName::from_static("content-length"), HeaderValue::from_static("1"));
            Ok(())
        },
    ));

    let mut response = client
        .execute(client.request(Method::GET, "/original-content-length-chunked").build())
        .await
        .unwrap();
    let first_error = response.bytes().await.unwrap_err();
    assert_eq!(first_error.kind, HttpErrorKind::ResponseBodyTooLarge);
    let repeated_error = response.bytes().await.unwrap_err();
    assert_eq!(repeated_error.kind, first_error.kind);
    server.finish().await;
}

#[tokio_test]
async fn test_request_url_can_differ_from_response_meta_url() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("valid options should create client");
    let rewritten_url =
        url::Url::parse("https://interceptor.example/rewritten").expect("static interceptor URL should parse");
    let rewritten_url_for_interceptor = rewritten_url.clone();
    client.add_response_interceptor(HttpResponseInterceptor::new(
        move |context: &mut HttpResponseInterceptorContext| {
            context.set_url(rewritten_url_for_interceptor.clone());
            Ok(())
        },
    ));

    let request = client.request(Method::GET, "/request-url-diff").build();
    let response = client.execute(request).await.expect("request should succeed");
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

#[tokio_test]
async fn test_request_url_includes_builder_query_params() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("valid options should create client");

    let request = client
        .request(Method::GET, "/request-url-query?existing=1")
        .query_param("added", "two words")
        .build();
    let response = client.execute(request).await.expect("request should succeed");
    let mut expected_request_url = server
        .base_url()
        .join("request-url-query?existing=1")
        .expect("request URL should join");
    expected_request_url.query_pairs_mut().append_pair("added", "two words");

    assert_eq!(response.request_url(), &expected_request_url);

    let captured = server.finish().await;
    assert_eq!(captured.target, "/request-url-query?existing=1&added=two+words");
}

#[tokio_test]
async fn test_status_error_url_includes_builder_query_params() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
        headers: vec![],
        body: b"retry later".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("valid options should create client");

    let request = client
        .request(Method::GET, "/status-query")
        .query_param("attempt", "1")
        .build();
    let error = client
        .execute(request)
        .await
        .expect_err("status should be returned as error");
    let expected_url = server
        .base_url()
        .join("status-query?attempt=1")
        .expect("request URL should join");

    assert_eq!(error.kind, HttpErrorKind::Status);
    assert_eq!(error.url, Some(expected_url));

    let captured = server.finish().await;
    assert_eq!(captured.target, "/status-query?attempt=1");
}

#[tokio_test]
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
    let mut client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("valid options should create client");
    let interceptor_url = url::Url::parse("https://interceptor.example/context-url-rewritten")
        .expect("static interceptor URL should parse");
    let expected_request_url = server
        .base_url()
        .join("context-url-timeout")
        .expect("request URL should join");
    let expected_url_for_interceptor = interceptor_url.clone();
    client.add_response_interceptor(HttpResponseInterceptor::new(
        move |context: &mut HttpResponseInterceptorContext| {
            context.set_url(expected_url_for_interceptor.clone());
            Ok(())
        },
    ));

    let request = client.request(Method::GET, "/context-url-timeout").build();
    let mut response = client.execute(request).await.expect("request should start");
    let error = response.bytes().await.expect_err("buffered read should timeout");

    assert_eq!(error.kind, HttpErrorKind::ReadTimeout);
    assert_eq!(error.url, Some(expected_request_url));
    let captured = server.finish().await;
    assert_eq!(captured.target, "/context-url-timeout");
}

#[tokio_test]
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
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }));

    let request = client.request(Method::GET, "/retry-status-filter").build();
    let error = client.execute(request).await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Status);
    let inner = retry_abort_inner_http(&error);
    assert_eq!(inner.kind, HttpErrorKind::Status);
    assert_eq!(inner.status, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
}

#[tokio_test]
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
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        attempts_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }));

    let request = client.request(Method::GET, "/retry-status-allow").build();
    let error = client.execute(request).await.unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Transport);
    let inner = retry_abort_inner_http(&error);
    assert_eq!(inner.kind, HttpErrorKind::Transport);
    assert_eq!(attempts.load(Ordering::Relaxed), 2);
}

#[tokio_test]
async fn test_retry_error_kind_allowlist_can_disable_transport_retry() {
    let mut options = HttpClientOptions::default();
    options.origin_policy = HttpOriginPolicy::AnyOrigin;
    options.logging.enabled = false;
    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.retry_error_kinds = Some(vec![HttpErrorKind::ReadTimeout]);
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_for_interceptor = Arc::clone(&attempts);
    client.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
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
    let inner = retry_abort_inner_http(&error);
    assert_eq!(inner.kind, HttpErrorKind::Transport);
    assert_eq!(attempts.load(Ordering::Relaxed), 1);
}

#[tokio_test]
async fn test_add_header_applies_client_default_header() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client.add_header("x-client", "default").unwrap();

    let request = client.request(Method::GET, "/default-header").build();
    let _ = client.execute(request).await.unwrap();
    let captured = server.finish().await;
    assert_eq!(captured.headers.get("x-client"), Some(&"default".to_string()));
}

#[tokio_test]
async fn test_add_headers_is_atomic_and_request_header_still_overrides() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client
        .add_headers(&[
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
    assert_eq!(captured.headers.get("x-batch-a"), Some(&"value-a".to_string()));
    assert_eq!(captured.headers.get("x-batch-b"), Some(&"value-b".to_string()));
    assert_eq!(captured.headers.get("x-order"), Some(&"request".to_string()));
}

#[tokio_test]
async fn test_add_headers_invalid_batch_does_not_partially_apply() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();

    let error = client
        .add_headers(&[("x-valid", "kept-out"), ("bad header", "boom")])
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("Invalid header name"));

    let request = client.request(Method::GET, "/atomic").build();
    let _ = client.execute(request).await.unwrap();
    let captured = server.finish().await;
    assert!(!captured.headers.contains_key("x-valid"));
}

#[tokio_test]
async fn test_add_header_invalid_value_does_not_apply() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();

    let error = client.add_header("x-bad", "line1\nline2").unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("Invalid header value"));

    let request = client.request(Method::GET, "/invalid-value").build();
    let _ = client.execute(request).await.unwrap();
    let captured = server.finish().await;
    assert!(!captured.headers.contains_key("x-bad"));
}

#[tokio_test]
async fn test_add_header_injector_still_overrides_client_default_header() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new().options(options).build().unwrap();
    client.add_header("x-order", "client").unwrap();
    client.add_header_injector(HttpHeaderInjector::new(|headers: &mut HeaderMap| {
        headers.insert(HeaderName::from_static("x-order"), HeaderValue::from_static("injector"));
        Ok(())
    }));

    let request = client.request(Method::GET, "/injector-overrides").build();
    let _ = client.execute(request).await.unwrap();
    let captured = server.finish().await;
    assert_eq!(captured.headers.get("x-order"), Some(&"injector".to_string()));
}

#[tokio_test]
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

    let mut client_options = HttpClientOptions::default();
    client_options.origin_policy = HttpOriginPolicy::AnyOrigin;
    let mut client = HttpClientBuilder::new().options(client_options).build().unwrap();
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

    assert_eq!(captured_original.headers.get("x-shared"), Some(&"base".to_string()));
    assert_eq!(captured_clone.headers.get("x-shared"), Some(&"base".to_string()));
    assert_eq!(captured_original.headers.get("x-origin-only"), Some(&"yes".to_string()));
    assert!(!captured_original.headers.contains_key("x-clone-only"));
    assert_eq!(captured_clone.headers.get("x-clone-only"), Some(&"yes".to_string()));
    assert!(!captured_clone.headers.contains_key("x-origin-only"));
}

#[tokio_test]
async fn test_review_contract_retry_after_error_and_success_interceptor() {
    let error_server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 429,
        headers: vec![("Retry-After".to_string(), "2".to_string())],
        body: b"slow down".to_vec(),
    })
    .await;
    let success_server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![("Retry-After".to_string(), "2".to_string())],
        body: b"ok".to_vec(),
    })
    .await;
    let error_calls = Arc::new(AtomicUsize::new(0));
    let success_calls = Arc::new(AtomicUsize::new(0));

    let mut error_options = HttpClientOptions::default();
    error_options.base_url = Some(error_server.base_url());
    let mut error_client = HttpClientBuilder::new().options(error_options).build().unwrap();
    let error_calls_for_interceptor = Arc::clone(&error_calls);
    error_client.add_response_interceptor(HttpResponseInterceptor::new(
        move |_context: &mut HttpResponseInterceptorContext| {
            error_calls_for_interceptor.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
    ));
    let error = error_client
        .execute(error_client.request(Method::GET, "/retry-after").build())
        .await
        .expect_err("429 should be returned through HttpError");
    assert_eq!(error.status, Some(StatusCode::TOO_MANY_REQUESTS));
    assert_eq!(error.retry_after, Some(Duration::from_secs(2)));
    assert_eq!(error_calls.load(Ordering::Relaxed), 0);
    error_server.finish().await;

    let mut success_options = HttpClientOptions::default();
    success_options.base_url = Some(success_server.base_url());
    let mut success_client = HttpClientBuilder::new().options(success_options).build().unwrap();
    let success_calls_for_interceptor = Arc::clone(&success_calls);
    success_client.add_response_interceptor(HttpResponseInterceptor::new(
        move |_context: &mut HttpResponseInterceptorContext| {
            success_calls_for_interceptor.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
    ));
    let response = success_client
        .execute(success_client.request(Method::GET, "/retry-after").build())
        .await
        .expect("200 should succeed");
    assert_eq!(response.retry_after_hint(), None);
    assert_eq!(success_calls.load(Ordering::Relaxed), 1);
    success_server.finish().await;
}

#[tokio_test]
async fn test_review_contract_cross_client_snapshot() {
    let originating_server = spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 503,
            headers: vec![],
            body: b"retry".to_vec(),
        },
        ResponsePlan::Immediate {
            status: 200,
            headers: vec![],
            body: b"origin".to_vec(),
        },
    ])
    .await;
    let mut origin_options = HttpClientOptions::default();
    origin_options.base_url = Some(originating_server.base_url());
    origin_options.origin_policy = HttpOriginPolicy::AnyOrigin;
    let mut origin = HttpClientBuilder::new().options(origin_options).build().unwrap();
    origin.add_header("x-client", "origin").unwrap();
    origin.add_header_injector(HttpHeaderInjector::new(|headers: &mut HeaderMap| {
        headers.insert("x-injector", HeaderValue::from_static("origin"));
        Ok(())
    }));
    let request = origin.request(Method::GET, "/snapshot").build();

    let mut executor_options = HttpClientOptions::default();
    executor_options.base_url = Some(url::Url::parse("http://127.0.0.1:1/").unwrap());
    executor_options.retry.enabled = true;
    executor_options.retry.max_attempts = 2;
    executor_options.retry.backoff = BackoffPolicy::immediate();
    let mut executor = HttpClientBuilder::new().options(executor_options).build().unwrap();
    executor.add_header("x-client", "executor").unwrap();
    let request_calls = Arc::new(AtomicUsize::new(0));
    let request_calls_for_interceptor = Arc::clone(&request_calls);
    executor.add_request_interceptor(HttpRequestInterceptor::new(move |_request: &mut HttpRequest| {
        request_calls_for_interceptor.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }));
    let response_calls = Arc::new(AtomicUsize::new(0));
    let response_calls_for_interceptor = Arc::clone(&response_calls);
    executor.add_response_interceptor(HttpResponseInterceptor::new(
        move |_context: &mut HttpResponseInterceptorContext| {
            response_calls_for_interceptor.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
    ));

    let response = executor.execute(request).await.expect("executor should send snapshot");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(request_calls.load(Ordering::Relaxed), 2);
    assert_eq!(response_calls.load(Ordering::Relaxed), 1);
    let captured = originating_server.finish().await;
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].target, "/snapshot");
    assert_eq!(captured[1].target, "/snapshot");
    assert_eq!(captured[1].headers.get("x-client").map(String::as_str), Some("origin"));
    assert_eq!(
        captured[1].headers.get("x-injector").map(String::as_str),
        Some("origin")
    );

    let redirect_target = TcpListener::bind("127.0.0.1:0").unwrap();
    redirect_target.set_nonblocking(true).unwrap();
    let target_url = format!("http://{}/target", redirect_target.local_addr().unwrap());
    let redirect_origin = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 302,
        headers: vec![("Location".to_string(), target_url)],
        body: vec![],
    })
    .await;
    let redirect_request = origin
        .request(Method::GET, &format!("{}redirect", redirect_origin.base_url()))
        .build();
    let redirect_error = executor
        .execute(redirect_request)
        .await
        .expect_err("executor SameOrigin policy should reject cross-origin redirect");
    assert_eq!(redirect_error.kind, HttpErrorKind::OriginPolicy);
    redirect_origin.finish().await;
    assert_eq!(redirect_target.accept().unwrap_err().kind(), ErrorKind::WouldBlock);
}

#[tokio_test]
async fn test_review_contract_rebuild_does_not_rebind_old_request() {
    let old_server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"old".to_vec(),
    })
    .await;
    let new_server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"new".to_vec(),
    })
    .await;
    let mut old_options = HttpClientOptions::default();
    old_options.base_url = Some(old_server.base_url());
    let old_client = HttpClientBuilder::new().options(old_options).build().unwrap();
    let old_request = old_client.request(Method::GET, "/old-request").build();

    let mut new_options = old_client.options().clone();
    new_options.base_url = Some(new_server.base_url());
    let new_client = old_client.rebuild_with_options(new_options).unwrap();
    new_client.execute(old_request).await.unwrap();
    assert_eq!(old_server.finish().await.target, "/old-request");

    new_client
        .execute(new_client.request(Method::GET, "/new-request").build())
        .await
        .unwrap();
    assert_eq!(new_server.finish().await.target, "/new-request");
}
