/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::sync::{Arc, Mutex};

use http::{HeaderMap, HeaderValue, Method, StatusCode};
use qubit_function::MutatingFunction;
use qubit_http::{
    HttpError, HttpErrorKind, HttpResponseInterceptor, HttpResponseInterceptors, HttpResponseMeta,
};
use url::Url;

#[test]
fn test_response_interceptor_apply_receives_context() {
    let seen = Arc::new(Mutex::new(None));
    let seen_for_interceptor = Arc::clone(&seen);
    let interceptor = HttpResponseInterceptor::new(move |meta| {
        let header = meta
            .headers
            .get("x-check")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        *seen_for_interceptor
            .lock()
            .expect("lock seen context in response interceptor") =
            Some((meta.status, meta.url.clone(), header));
        meta.url = Url::parse("https://example.test/rewritten").expect("valid rewritten URL");
        Ok(())
    });

    let mut headers = HeaderMap::new();
    headers.insert("x-check", HeaderValue::from_static("ok"));
    let mut meta = HttpResponseMeta::new(
        StatusCode::CREATED,
        headers,
        Url::parse("https://example.test/path").expect("valid test URL"),
        Method::GET,
    );
    interceptor
        .apply(&mut meta)
        .expect("response interceptor apply should succeed");

    let seen = seen
        .lock()
        .expect("lock seen context for assertion")
        .clone()
        .expect("response interceptor should capture context");
    assert_eq!(seen.0, StatusCode::CREATED);
    assert_eq!(seen.1, Url::parse("https://example.test/path").unwrap());
    assert_eq!(seen.2, "ok");
    assert_eq!(
        meta.url,
        Url::parse("https://example.test/rewritten").unwrap()
    );
}

#[test]
fn test_response_interceptor_apply_propagates_error() {
    let interceptor =
        HttpResponseInterceptor::new(|_meta| Err(HttpError::other("response interceptor failure")));
    let mut meta = HttpResponseMeta::new(
        StatusCode::OK,
        HeaderMap::new(),
        Url::parse("https://example.test/").expect("valid test URL"),
        Method::GET,
    );

    let error = interceptor
        .apply(&mut meta)
        .expect_err("response interceptor should propagate callback errors");
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("response interceptor failure"));
}

#[test]
fn test_response_interceptors_apply_enriches_error_context() {
    let mut interceptors = HttpResponseInterceptors::new();
    interceptors.push(HttpResponseInterceptor::new(|_meta| {
        Err(HttpError::other("response interceptor list failure"))
    }));
    let mut meta = HttpResponseMeta::new(
        StatusCode::ACCEPTED,
        HeaderMap::new(),
        Url::parse("https://example.test/list").expect("valid test URL"),
        Method::POST,
    );

    let error = interceptors
        .apply(&mut meta)
        .expect_err("interceptor list should propagate callback errors");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert_eq!(error.status, Some(StatusCode::ACCEPTED));
    assert_eq!(error.method, Some(Method::POST));
    assert_eq!(
        error.url,
        Some(Url::parse("https://example.test/list").unwrap())
    );
}

#[test]
fn test_response_interceptors_apply_preserves_existing_error_context() {
    let existing_url = Url::parse("https://example.test/existing").expect("valid test URL");
    let mut interceptors = HttpResponseInterceptors::new();
    interceptors.push(HttpResponseInterceptor::new({
        let existing_url = existing_url.clone();
        move |_meta| {
            Err(HttpError::other("response interceptor list failure")
                .with_status(StatusCode::BAD_GATEWAY)
                .with_method(&Method::PATCH)
                .with_url(&existing_url))
        }
    }));
    let mut meta = HttpResponseMeta::new(
        StatusCode::ACCEPTED,
        HeaderMap::new(),
        Url::parse("https://example.test/list").expect("valid test URL"),
        Method::POST,
    );

    let error = interceptors
        .apply(&mut meta)
        .expect_err("interceptor list should propagate callback errors");

    assert_eq!(error.status, Some(StatusCode::BAD_GATEWAY));
    assert_eq!(error.method, Some(Method::PATCH));
    assert_eq!(error.url, Some(existing_url));
}

#[test]
fn test_response_interceptor_clone_and_debug() {
    let interceptor = HttpResponseInterceptor::new(|_meta| Ok(()));
    let cloned = interceptor.clone();

    let output = format!("{:?}", cloned);
    assert!(!output.is_empty(), "debug output should not be empty");
}
