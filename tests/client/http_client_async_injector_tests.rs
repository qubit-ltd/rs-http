// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use http::StatusCode;
use http::header::HeaderName;
use qubit_http::AsyncHttpHeaderInjector;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpError;
use qubit_http::HttpErrorKind;
use qubit_http::HttpHeaderInjector;
use tokio::test as tokio_test;
use tokio::time::timeout;
use url::Url;

use crate::common::ResponsePlan;
use crate::common::spawn_multi_shot_server;
use crate::common::spawn_one_shot_server;

fn injector_failure(context: u8) -> HttpError {
    let mut error = HttpError::other("injector failed").with_source(std::io::Error::other("source failure"));
    if context & 1 != 0 {
        error = error.with_method(&Method::POST);
    }
    if context & 2 != 0 {
        error = error.with_url(&Url::parse("https://upstream.example/token").unwrap());
    }
    if context & 4 != 0 {
        error = error.with_status(StatusCode::BAD_GATEWAY);
    }
    error
}

#[tokio_test]
async fn test_async_header_injector_runs_after_sync_injector_with_stable_order() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;

    let order = Arc::new(Mutex::new(Vec::<String>::new()));
    let sync_order = order.clone();
    let async_order = order.clone();

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");
    client.add_header_injector(HttpHeaderInjector::new(move |headers: &mut HeaderMap| {
        sync_order.lock().unwrap().push("sync".to_string());
        headers.insert(HeaderName::from_static("x-flow"), HeaderValue::from_static("sync"));
        Ok(())
    }));
    client.add_async_header_injector(AsyncHttpHeaderInjector::new(move |headers: &mut HeaderMap| {
        let async_order = async_order.clone();
        Box::pin(async move {
            async_order.lock().unwrap().push("async".to_string());
            headers.insert(HeaderName::from_static("x-flow"), HeaderValue::from_static("async"));
            Ok(())
        })
    }));

    let request = client.request(Method::GET, "/async-injector").build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert_eq!(captured.headers.get("x-flow"), Some(&"async".to_string()));
    assert_eq!(
        order.lock().unwrap().clone(),
        vec!["sync".to_string(), "async".to_string()]
    );
}

#[tokio_test]
async fn test_async_header_injector_failure_short_circuits_request() {
    for is_async in [false, true] {
        for context in 0..8 {
            let server = spawn_multi_shot_server(vec![]).await;
            let mut options = HttpClientOptions::default();
            options.base_url = Some(server.base_url());
            let mut client = HttpClientBuilder::new()
                .options(options)
                .build()
                .expect("client should be created");
            if is_async {
                client.add_async_header_injector(AsyncHttpHeaderInjector::new(move |_headers: &mut HeaderMap| {
                    let error = injector_failure(context);
                    Box::pin(async move { Err(error) })
                }));
            } else {
                client.add_header_injector(HttpHeaderInjector::new(move |_headers: &mut HeaderMap| {
                    Err(injector_failure(context))
                }));
            }

            let path = "/async-fail?access_token=context-secret";
            let request = client.request(Method::GET, path).build();
            let base_url = server.base_url();
            let expected_url = Url::parse(&format!("{}{}", base_url.as_str().trim_end_matches('/'), path)).unwrap();
            let error = timeout(Duration::from_secs(3), client.execute(request))
                .await
                .expect("execute timed out")
                .expect_err("request should fail on injector");
            assert_eq!(error.kind, HttpErrorKind::Other);
            assert!(error.message.contains("injector failed"));
            assert_eq!(
                error.method.as_ref(),
                Some(if context & 1 != 0 { &Method::POST } else { &Method::GET })
            );
            let expected_error_url = if context & 2 != 0 {
                Url::parse("https://upstream.example/token").unwrap()
            } else {
                expected_url
            };
            assert_eq!(error.url.as_ref(), Some(&expected_error_url));
            assert_eq!(error.status, (context & 4 != 0).then_some(StatusCode::BAD_GATEWAY));
            assert!(
                std::error::Error::source(&error)
                    .and_then(|source| source.downcast_ref::<std::io::Error>())
                    .is_some()
            );
            assert!(!error.to_string().contains("context-secret"));
            assert!(!format!("{error:?}").contains("context-secret"));

            let captured = timeout(Duration::from_secs(3), server.finish())
                .await
                .expect("server finish timed out");
            assert!(captured.is_empty());
        }
    }
}

#[tokio_test]
async fn test_injector_failure_context_survives_retry_projection() {
    let server = spawn_multi_shot_server(vec![]).await;
    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    options.retry.enabled = true;
    let mut client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");
    client.add_async_header_injector(AsyncHttpHeaderInjector::new(|_headers: &mut HeaderMap| {
        Box::pin(async move {
            Err(HttpError::other("retry injector failed").with_source(std::io::Error::other("retry source failure")))
        })
    }));
    let path = "/retry-fail?access_token=context-secret";
    let request = client.request(Method::GET, path).build();
    let base_url = server.base_url();
    let expected_url = Url::parse(&format!("{}{}", base_url.as_str().trim_end_matches('/'), path)).unwrap();

    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("injector should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert_eq!(error.method.as_ref(), Some(&Method::GET));
    assert_eq!(error.url.as_ref(), Some(&expected_url));
    let mut source = std::error::Error::source(&error);
    let mut found_io_error = false;
    while let Some(current) = source {
        found_io_error |= current.downcast_ref::<std::io::Error>().is_some();
        source = current.source();
    }
    assert!(
        found_io_error,
        "retry projection should retain the injector source chain"
    );
    assert_eq!(
        error.retry_diagnostics().map(|diagnostics| diagnostics.attempts()),
        Some(1)
    );
    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert!(captured.is_empty());
}

#[tokio_test]
async fn test_clear_async_header_injectors_removes_async_mutation() {
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
        .expect("client should be created");
    client.add_async_header_injector(AsyncHttpHeaderInjector::new(|headers: &mut HeaderMap| {
        Box::pin(async move {
            headers.insert(
                HeaderName::from_static("x-removed"),
                HeaderValue::from_static("present"),
            );
            Ok(())
        })
    }));
    client.clear_async_header_injectors();

    let request = client.request(Method::GET, "/async-clear").build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert!(!captured.headers.contains_key("x-removed"));
}
