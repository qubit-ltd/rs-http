/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use http::HeaderMap;
use http::HeaderValue;
use qubit_http::AsyncHttpHeaderInjector;

#[tokio::test]
async fn test_async_header_injector_apply_updates_header_map() {
    let injector = AsyncHttpHeaderInjector::new(|headers| {
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

    assert_eq!(
        headers
            .get("x-async")
            .expect("x-async header should be injected"),
        "ok"
    );
}

#[tokio::test]
async fn test_async_header_injector_apply_propagates_error() {
    let injector = AsyncHttpHeaderInjector::new(|_headers| {
        Box::pin(async move { Err(qubit_http::HttpError::other("injector failed")) })
    });

    let mut headers = HeaderMap::new();
    let error = injector
        .apply(&mut headers)
        .await
        .expect_err("injector should fail");
    assert_eq!(error.kind, qubit_http::HttpErrorKind::Other);
    assert!(error.message.contains("injector failed"));
}

#[tokio::test]
async fn test_async_header_injector_clone_keeps_same_behavior() {
    let injector = AsyncHttpHeaderInjector::new(|headers| {
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
    let injector = AsyncHttpHeaderInjector::new(|_headers| Box::pin(async move { Ok(()) }));
    let output = format!("{injector:?}");
    assert!(output.contains("AsyncHttpHeaderInjector"));
}
