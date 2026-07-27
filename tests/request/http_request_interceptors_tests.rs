// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use http::Method;
use qubit_http::{HttpClientFactory, HttpRequestInterceptor, HttpRequestInterceptors};

#[test]
fn test_http_request_interceptors_apply_in_insertion_order() {
    let client = HttpClientFactory::new()
        .create_default()
        .expect("default client should be created");
    let mut request = client.request(Method::GET, "https://example.com/").build();
    let mut interceptors = HttpRequestInterceptors::new();
    interceptors.push(HttpRequestInterceptor::new(
        |request: &mut qubit_http::HttpRequest| {
            request
                .set_header("x-first", "1")
                .expect("first header should be valid");
            Ok(())
        },
    ));
    interceptors.push(HttpRequestInterceptor::new(
        |request: &mut qubit_http::HttpRequest| {
            let first = request
                .headers()
                .get("x-first")
                .expect("first interceptor should already run")
                .to_str()
                .expect("header should be utf-8")
                .to_string();
            request
                .set_header("x-second", &first)
                .expect("second header should be valid");
            Ok(())
        },
    ));

    interceptors
        .apply(&mut request)
        .expect("interceptors should succeed");

    assert_eq!(request.headers().get("x-first").expect("x-first"), "1");
    assert_eq!(request.headers().get("x-second").expect("x-second"), "1");
}
