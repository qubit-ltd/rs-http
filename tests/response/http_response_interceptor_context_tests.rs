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
use http::header::SET_COOKIE;
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

#[test]
fn test_http_response_interceptor_context_debug_masks_sensitive_values() {
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, HeaderValue::from_static("session=debug-cookie-secret"));
    let context = HttpResponseInterceptorContext::new(
        StatusCode::OK,
        headers,
        Url::parse("https://debug-user:debug-url-secret@example.test/context?password=debug-password-secret&access_token=debug-token-secret")
            .expect("valid URL"),
        Method::GET,
    );

    let debug = format!("{context:?}");

    assert!(!debug.contains("debug-user"));
    assert!(!debug.contains("debug-url-secret"));
    assert!(!debug.contains("debug-password-secret"));
    assert!(!debug.contains("debug-token-secret"));
    assert!(!debug.contains("debug-cookie-secret"));
    assert!(debug.contains("%3Credacted%3E"));
    assert!(debug.contains("****"));
}
