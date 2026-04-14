/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::sync::{Arc, Mutex};

use http::{HeaderMap, HeaderValue, StatusCode};
use qubit_function::MutatingFunction;
use qubit_http::{HttpError, HttpErrorKind, HttpResponseMeta, ResponseInterceptor};
use url::Url;

#[test]
fn test_response_interceptor_apply_receives_context() {
    let seen = Arc::new(Mutex::new(None));
    let seen_for_interceptor = Arc::clone(&seen);
    let interceptor = ResponseInterceptor::new(move |meta| {
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
    let interceptor = ResponseInterceptor::new(|_meta| {
        Err(HttpError::other("response interceptor failure"))
    });
    let mut meta = HttpResponseMeta::new(
        StatusCode::OK,
        HeaderMap::new(),
        Url::parse("https://example.test/").expect("valid test URL"),
    );

    let error = interceptor
        .apply(&mut meta)
        .expect_err("response interceptor should propagate callback errors");
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("response interceptor failure"));
}

#[test]
fn test_response_interceptor_clone_and_debug() {
    let interceptor = ResponseInterceptor::new(|_meta| Ok(()));
    let cloned = interceptor.clone();

    let output = format!("{:?}", cloned);
    assert!(!output.is_empty(), "debug output should not be empty");
}
