// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use http::{
    HeaderMap,
    Method,
    StatusCode,
};
use qubit_http::{
    HttpResponseInterceptor,
    HttpResponseInterceptors,
    HttpResponseMeta,
};
use url::Url;

#[test]
fn test_http_response_interceptors_clear_removes_registered_callbacks() {
    let mut interceptors = HttpResponseInterceptors::new();
    interceptors.push(HttpResponseInterceptor::new(|context| {
        context
            .headers_mut()
            .insert("x-interceptor", http::HeaderValue::from_static("called"));
        Ok(())
    }));
    interceptors.clear();
    let mut meta = HttpResponseMeta::new(
        StatusCode::OK,
        HeaderMap::new(),
        Url::parse("https://example.com/").expect("valid URL"),
        Method::GET,
    );

    interceptors
        .apply(&mut meta)
        .expect("cleared interceptors should be no-op");

    assert_eq!(meta.status(), StatusCode::OK);
    assert!(!meta.headers().contains_key("x-interceptor"));
}
