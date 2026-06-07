// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use http::Method;
use qubit_http::{
    HttpClientFactory,
    HttpError,
    HttpErrorKind,
    HttpRequestInterceptor,
    HttpRequestInterceptors,
};
use url::Url;

#[test]
fn test_request_interceptors_apply_uses_parsed_path_when_resolved_url_cache_missing(
) {
    let client = HttpClientFactory::new()
        .create_default()
        .expect("default options should create client");
    let mut request =
        client.request(Method::GET, "http://[::1]/resource").build();
    request.set_ipv4_only(true);
    assert!(request.resolved_url().is_err());

    let mut interceptors = HttpRequestInterceptors::new();
    interceptors.push(HttpRequestInterceptor::new(|_request| {
        Err(HttpError::other("request interceptor failed unexpectedly"))
    }));

    let error = interceptors
        .apply(&mut request)
        .expect_err("interceptor failure should be propagated");
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert_eq!(error.method, Some(Method::GET));
    assert_eq!(
        error.url,
        Some(Url::parse("http://[::1]/resource").unwrap())
    );
}

#[test]
fn test_request_interceptors_apply_keeps_url_empty_when_path_is_not_absolute() {
    let client = HttpClientFactory::new()
        .create_default()
        .expect("default options should create client");
    let mut request = client.request(Method::GET, "/relative-only").build();
    assert!(request.resolved_url().is_err());

    let mut interceptors = HttpRequestInterceptors::new();
    interceptors.push(HttpRequestInterceptor::new(|_request| {
        Err(HttpError::other("request interceptor failed"))
    }));

    let error = interceptors
        .apply(&mut request)
        .expect_err("interceptor failure should be propagated");
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert_eq!(error.method, Some(Method::GET));
    assert!(error.url.is_none());
}

#[test]
fn test_request_interceptors_apply_preserves_existing_error_url() {
    let client = HttpClientFactory::new()
        .create_default()
        .expect("default options should create client");
    let mut request = client.request(Method::GET, "/relative-only").build();
    let expected_url =
        Url::parse("https://upstream.example/interceptor-failed")
            .expect("URL should parse");

    let mut interceptors = HttpRequestInterceptors::new();
    interceptors.push(HttpRequestInterceptor::new({
        let expected_url = expected_url.clone();
        move |_request| {
            Err(HttpError::other("request interceptor failed")
                .with_url(&expected_url))
        }
    }));

    let error = interceptors
        .apply(&mut request)
        .expect_err("interceptor failure should be propagated");
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert_eq!(error.method, Some(Method::GET));
    assert_eq!(error.url, Some(expected_url));
}
