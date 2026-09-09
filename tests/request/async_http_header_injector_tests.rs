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
use tokio::task;
use tokio::test as tokio_test;

#[tokio_test]
async fn test_async_http_header_injector_runs_async_mutation() {
    let injector = AsyncHttpHeaderInjector::new(|headers: &mut HeaderMap| {
        Box::pin(async move {
            task::yield_now().await;
            headers.insert("x-token", HeaderValue::from_static("fresh"));
            Ok(())
        })
    });

    let mut headers = HeaderMap::new();
    injector
        .apply(&mut headers)
        .await
        .expect("injector should mutate headers");

    assert_eq!(headers.get("x-token").expect("token header"), "fresh");
}
