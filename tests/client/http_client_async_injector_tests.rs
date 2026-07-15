// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::{
    Arc,
    Mutex,
};
use std::time::Duration;

use http::header::HeaderName;
use http::{
    HeaderValue,
    Method,
    StatusCode,
};
use qubit_http::{
    AsyncHttpHeaderInjector,
    HttpClientFactory,
    HttpClientOptions,
    HttpHeaderInjector,
};
use tokio::time::timeout;

use crate::common::{
    spawn_multi_shot_server,
    spawn_one_shot_server,
    ResponsePlan,
};

#[tokio::test]
async fn test_async_header_injector_runs_after_sync_injector_with_stable_order()
{
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
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    client.add_header_injector(HttpHeaderInjector::new(
        move |headers: &mut http::HeaderMap| {
            sync_order.lock().unwrap().push("sync".to_string());
            headers.insert(
                HeaderName::from_static("x-flow"),
                HeaderValue::from_static("sync"),
            );
            Ok(())
        },
    ));
    client.add_async_header_injector(AsyncHttpHeaderInjector::new(
        move |headers: &mut http::HeaderMap| {
            let async_order = async_order.clone();
            Box::pin(async move {
                async_order.lock().unwrap().push("async".to_string());
                headers.insert(
                    HeaderName::from_static("x-flow"),
                    HeaderValue::from_static("async"),
                );
                Ok(())
            })
        },
    ));

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

#[tokio::test]
async fn test_async_header_injector_failure_short_circuits_request() {
    let server = spawn_multi_shot_server(vec![]).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(server.base_url());
    let mut client = HttpClientFactory::new()
        .create(options)
        .expect("client should be created");
    client.add_async_header_injector(AsyncHttpHeaderInjector::new(
        |_headers: &mut http::HeaderMap| {
            Box::pin(async move {
                Err(qubit_http::HttpError::other("async injector failed"))
            })
        },
    ));

    let request = client.request(Method::GET, "/async-fail").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .expect_err("request should fail on async injector");
    assert_eq!(error.kind, qubit_http::HttpErrorKind::Other);
    assert!(error.message.contains("async injector failed"));

    let captured = timeout(Duration::from_secs(3), server.finish())
        .await
        .expect("server finish timed out");
    assert!(captured.is_empty());
}

#[tokio::test]
async fn test_clear_async_header_injectors_removes_async_mutation() {
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
        .expect("client should be created");
    client.add_async_header_injector(AsyncHttpHeaderInjector::new(
        |headers: &mut http::HeaderMap| {
            Box::pin(async move {
                headers.insert(
                    HeaderName::from_static("x-removed"),
                    HeaderValue::from_static("present"),
                );
                Ok(())
            })
        },
    ));
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
