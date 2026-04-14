/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::sync::{Arc, Mutex};

use http::{HeaderMap, HeaderValue, Method, StatusCode};
use qubit_http::{HttpError, HttpErrorKind, ResponseInterceptor};
use url::Url;

#[test]
fn test_response_interceptor_apply_receives_context() {
    let seen = Arc::new(Mutex::new(None));
    let seen_for_interceptor = Arc::clone(&seen);
    let interceptor = ResponseInterceptor::new(move |status, headers, method, url| {
        let header = headers
            .get("x-check")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        *seen_for_interceptor
            .lock()
            .expect("lock seen context in response interceptor") =
            Some((status, method.clone(), url.clone(), header));
        Ok(())
    });

    let mut headers = HeaderMap::new();
    headers.insert("x-check", HeaderValue::from_static("ok"));
    let method = Method::POST;
    let url = Url::parse("https://example.test/path").expect("valid test URL");
    interceptor
        .apply(StatusCode::CREATED, &headers, &method, &url)
        .expect("response interceptor apply should succeed");

    let seen = seen
        .lock()
        .expect("lock seen context for assertion")
        .clone()
        .expect("response interceptor should capture context");
    assert_eq!(seen.0, StatusCode::CREATED);
    assert_eq!(seen.1, Method::POST);
    assert_eq!(seen.2, url);
    assert_eq!(seen.3, "ok");
}

#[test]
fn test_response_interceptor_apply_propagates_error() {
    let interceptor = ResponseInterceptor::new(|_status, _headers, _method, _url| {
        Err(HttpError::other("response interceptor failure"))
    });

    let error = interceptor
        .apply(
            StatusCode::OK,
            &HeaderMap::new(),
            &Method::GET,
            &Url::parse("https://example.test/").expect("valid test URL"),
        )
        .expect_err("response interceptor should propagate callback errors");
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("response interceptor failure"));
}

#[test]
fn test_response_interceptor_clone_and_debug() {
    let interceptor = ResponseInterceptor::new(|_status, _headers, _method, _url| Ok(()));
    let cloned = interceptor.clone();

    let output = format!("{:?}", cloned);
    assert!(output.contains("ResponseInterceptor"));
    assert!(
        output.contains(".."),
        "debug output should be non-exhaustive and hide closure internals"
    );
}
