/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::time::Duration;

use http::header::RETRY_AFTER;
use http::{
    HeaderMap,
    HeaderValue,
    Method,
    StatusCode,
};
use qubit_http::{
    HttpResponseInterceptorContext,
    HttpResponseMeta,
};
use url::Url;

#[test]
fn test_http_response_interceptor_context_exposes_immutable_status_and_method() {
    let context = HttpResponseInterceptorContext::new(
        StatusCode::ACCEPTED,
        HeaderMap::new(),
        Url::parse("https://example.test/context").expect("valid URL"),
        Method::POST,
    );

    assert_eq!(context.status(), StatusCode::ACCEPTED);
    assert_eq!(context.method(), &Method::POST);
    assert_eq!(
        context.url(),
        &Url::parse("https://example.test/context").expect("valid URL")
    );
}

#[test]
fn test_http_response_interceptor_context_mutates_headers_and_url() {
    let mut context = HttpResponseInterceptorContext::new(
        StatusCode::OK,
        HeaderMap::new(),
        Url::parse("https://example.test/original").expect("valid URL"),
        Method::GET,
    );

    context
        .headers_mut()
        .insert("x-intercepted", HeaderValue::from_static("yes"));
    context.set_url(Url::parse("https://example.test/rewritten").expect("valid URL"));

    assert_eq!(
        context
            .headers()
            .get("x-intercepted")
            .expect("header should be inserted"),
        "yes"
    );
    assert_eq!(
        context.url(),
        &Url::parse("https://example.test/rewritten").expect("valid URL")
    );
}

#[test]
fn test_http_response_interceptor_context_from_meta_preserves_retry_after_hint() {
    let mut headers = HeaderMap::new();
    headers.insert(RETRY_AFTER, HeaderValue::from_static("3"));
    let meta = HttpResponseMeta::new(
        StatusCode::SERVICE_UNAVAILABLE,
        headers,
        Url::parse("https://example.test/retry").expect("valid URL"),
        Method::GET,
    );

    let context = HttpResponseInterceptorContext::from_meta(&meta);

    assert_eq!(context.retry_after_hint(), Some(Duration::from_secs(3)));
}
