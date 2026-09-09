// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use http::HeaderMap;
use http::HeaderValue;
use qubit_http::AsyncHttpHeaderInjector;
use qubit_http::HttpError;
use qubit_http::HttpErrorKind;
use tokio::test as tokio_test;

#[tokio_test]
async fn test_async_header_injector_apply_updates_header_map() {
    let injector = AsyncHttpHeaderInjector::new(|headers: &mut HeaderMap| {
        Box::pin(async move {
            headers.insert("x-async", HeaderValue::from_static("ok"));
            Ok(())
        })
    });

    let mut headers = HeaderMap::new();
    injector
        .apply(&mut headers)
        .await
        .expect("async injector should succeed");

    assert_eq!(headers.get("x-async").expect("x-async header should be injected"), "ok");
}

#[tokio_test]
async fn test_async_header_injector_apply_propagates_error() {
    let injector = AsyncHttpHeaderInjector::new(|_headers: &mut HeaderMap| {
        Box::pin(async move { Err(HttpError::other("injector failed")) })
    });

    let mut headers = HeaderMap::new();
    let error = injector.apply(&mut headers).await.expect_err("injector should fail");
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("injector failed"));
}

#[tokio_test]
async fn test_async_header_injector_clone_keeps_same_behavior() {
    let injector = AsyncHttpHeaderInjector::new(|headers: &mut HeaderMap| {
        Box::pin(async move {
            headers.insert("x-clone", HeaderValue::from_static("ok"));
            Ok(())
        })
    });
    let cloned = injector.clone();

    let mut headers = HeaderMap::new();
    cloned
        .apply(&mut headers)
        .await
        .expect("cloned injector should succeed");
    assert_eq!(
        headers
            .get("x-clone")
            .expect("x-clone header should exist after clone apply"),
        "ok"
    );
}

#[test]
fn test_async_header_injector_debug_output_contains_type_name() {
    let injector = AsyncHttpHeaderInjector::new(|_headers: &mut HeaderMap| Box::pin(async move { Ok(()) }));
    let output = format!("{injector:?}");
    assert!(output.contains("AsyncHttpHeaderInjector"));
}
