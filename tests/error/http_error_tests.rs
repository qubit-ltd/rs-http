// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error;

use http::StatusCode;
use qubit_http::{
    HttpError,
    HttpErrorKind,
    LogRedactionPolicy,
    RetryHint,
};
use qubit_redact::{
    http::UrlPathPolicy,
    Sensitivity,
};

#[test]
fn test_http_error_builder_methods() {
    let url = url::Url::parse("https://example.com/test").unwrap();
    let error = HttpError::new(HttpErrorKind::Decode, "decode failure")
        .with_method(&http::Method::POST)
        .with_url(&url)
        .with_status(StatusCode::BAD_GATEWAY);

    assert_eq!(error.kind, HttpErrorKind::Decode);
    assert_eq!(error.method, Some(http::Method::POST));
    assert_eq!(error.url, Some(url));
    assert_eq!(error.status, Some(StatusCode::BAD_GATEWAY));
    assert!(error.message.contains("decode failure"));
}

#[test]
fn test_http_error_debug_masks_sensitive_url_values() {
    let url = url::Url::parse(
        "https://debug-user:debug-url-secret@example.com/path?accessToken=debug-query-secret#debug-fragment-secret",
    )
    .expect("URL should parse");
    let error = HttpError::transport(
        "transport failed for https://debug-user:debug-url-secret@example.com/path?accessToken=debug-query-secret#debug-fragment-secret",
    )
    .with_method(&http::Method::GET)
    .with_url(&url);

    let debug = format!("{error:?}");

    assert!(!debug.contains("debug-user"));
    assert!(!debug.contains("debug-url-secret"));
    assert!(!debug.contains("debug-query-secret"));
    assert!(!debug.contains("debug-fragment-secret"));
    assert!(debug.contains("****"));
}

#[test]
fn test_http_error_debug_redacts_url_tokens_with_punctuation() {
    let error = HttpError::transport(
        "failed (https://debug-user:debug-url-secret@example.com/path?accessToken=debug-query-secret). retry http://debug-user:debug-url-secret@example.com/next?clientSecret=debug-query-secret!",
    );

    let debug = format!("{error:?}");

    assert!(!debug.contains("debug-user"));
    assert!(!debug.contains("debug-url-secret"));
    assert!(!debug.contains("debug-query-secret"));
    assert!(debug.contains("****"));
}

#[test]
fn test_http_error_debug_redacts_url_schemes_inside_token() {
    let error = HttpError::transport(
        "failed prefixhttps://debug-user:debug-url-secret@example.com/path?accessToken=debug-query-secret/http://second-user:second-secret@second.example/private",
    );

    let debug = format!("{error:?}");

    assert!(!debug.contains("debug-user"));
    assert!(!debug.contains("debug-url-secret"));
    assert!(!debug.contains("debug-query-secret"));
    assert!(!debug.contains("second-user"));
    assert!(!debug.contains("second-secret"));
    assert!(debug.contains("prefixhttps://****:%3Credacted%3E@example.com"));
    assert!(debug.contains("http://****:%3Credacted%3E@second.example"));
}

#[test]
fn test_http_error_debug_redacts_uppercase_url_scheme_tokens() {
    let error = HttpError::transport(
        "failed HTTPS://debug-user:debug-url-secret@example.com/path?accessToken=debug-query-secret.",
    );

    let debug = format!("{error:?}");

    assert!(!debug.contains("debug-user"));
    assert!(!debug.contains("debug-url-secret"));
    assert!(!debug.contains("debug-query-secret"));
    assert!(debug.contains("://****:%3Credacted%3E@example.com"));
}

#[test]
fn test_http_error_debug_uses_custom_log_redaction_policy() {
    let policy = LogRedactionPolicy::builder()
        .raise_query("customer_secret", Sensitivity::High)
        .build()
        .expect("log redaction policy should be valid");
    let url =
        url::Url::parse("https://example.com/path?customer_secret=debug-custom-secret&visible=ok")
            .expect("URL should parse");
    let error = HttpError::transport(
        "transport failed for https://example.com/path?customer_secret=debug-custom-secret&visible=ok",
    )
    .with_url(&url)
    .with_log_redaction_policy(policy);

    let debug = format!("{error:?}");

    assert!(!debug.contains("debug-custom-secret"));
    assert!(debug.contains("customer_secret=****"));
}

#[test]
fn test_http_error_display_masks_sensitive_url_values() {
    let error = HttpError::transport(
        "transport failed for https://display-user:display-password@example.com/path?access_token=display-secret",
    );

    let display = error.to_string();

    assert!(!display.contains("display-user"));
    assert!(!display.contains("display-password"));
    assert!(!display.contains("display-secret"));
    assert!(display.contains("****"));
}

#[test]
fn test_http_error_display_uses_custom_log_redaction_policy() {
    let policy = LogRedactionPolicy::builder()
        .raise_query("customer_secret", Sensitivity::High)
        .build()
        .expect("log redaction policy should be valid");
    let error = HttpError::transport(
        "transport failed for https://example.com/path?customer_secret=display-custom-secret&visible=ok",
    )
    .with_log_redaction_policy(policy);

    let display = error.to_string();

    assert!(!display.contains("display-custom-secret"));
    assert!(display.contains("customer_secret=****"));
}

#[test]
fn test_http_error_debug_honors_url_path_redaction_policy() {
    let raw_url = "https://alice:error-password-secret@example.com/tenant/error-path-secret?access_token=error-query-secret#error-fragment-secret";
    let url = url::Url::parse(raw_url).expect("URL should parse");
    let error = HttpError::transport(format!("transport failed for {raw_url}"))
        .with_url(&url)
        .with_log_redaction_policy(
            LogRedactionPolicy::builder()
                .url_path_policy(UrlPathPolicy::Redact)
                .build()
                .expect("log redaction policy should be valid"),
        );

    let debug = format!("{error:?}");

    assert!(!debug.contains("tenant/error-path-secret"));
    assert!(!debug.contains("alice"));
    assert!(!debug.contains("error-password-secret"));
    assert!(!debug.contains("error-query-secret"));
    assert!(!debug.contains("error-fragment-secret"));
    assert!(debug.contains("/%3Credacted%3E?"));
}

#[test]
fn test_http_error_debug_redacts_incomplete_url_like_tokens() {
    let error = HttpError::transport("failed near https:// and plain text");

    let debug = format!("{error:?}");

    assert!(debug.contains("<redacted: invalid URL>"));
    assert!(!debug.contains("https://"));
    assert!(debug.contains("plain"));
}

#[test]
fn test_http_error_debug_keeps_arbitrary_non_url_message_text() {
    let error = HttpError::transport("opaque diagnostic message");

    let debug = format!("{error:?}");

    assert!(debug.contains("opaque diagnostic message"));
}

#[test]
fn test_http_error_debug_redacts_invalid_url_token() {
    let error = HttpError::transport(
        "failed near https://debug-user:debug-password@example.com:bad-port/path?access_token=query-secret).",
    );

    let debug = format!("{error:?}");

    assert!(debug.contains("<redacted: invalid URL>)."));
    assert!(!debug.contains("debug-user"));
    assert!(!debug.contains("debug-password"));
    assert!(!debug.contains("query-secret"));
}

#[test]
fn test_http_error_debug_preserves_diagnostic_message_whitespace() {
    let error = HttpError::transport(
        "first line\tthen https://debug-user:debug-url-secret@example.com/path?accessToken=debug-query-secret\nsecond line",
    );

    let debug = format!("{error:?}");

    assert!(debug.contains("\\tthen"));
    assert!(debug.contains("\\nsecond line"));
    assert!(!debug.contains("debug-user"));
    assert!(!debug.contains("debug-url-secret"));
    assert!(!debug.contains("debug-query-secret"));
}

#[test]
fn test_http_error_build_client_constructor() {
    let error = HttpError::build_client("builder failed");
    assert_eq!(error.kind, HttpErrorKind::BuildClient);
    assert_eq!(error.message, "builder failed");
}

#[test]
fn test_http_error_request_timeout_constructor() {
    let error = HttpError::request_timeout("request timed out");
    assert_eq!(error.kind, HttpErrorKind::RequestTimeout);
    assert_eq!(error.message, "request timed out");
}

#[test]
fn test_http_error_retry_layer_constructors_and_retry_hints() {
    let attempt = HttpError::retry_attempt_timeout("attempt budget");
    assert_eq!(attempt.kind, HttpErrorKind::RetryAttemptTimeout);
    assert_eq!(attempt.retry_hint(), RetryHint::NonRetryable);

    let budget = HttpError::retry_max_elapsed_exceeded("max elapsed");
    assert_eq!(budget.kind, HttpErrorKind::RetryMaxElapsedExceeded);
    assert_eq!(budget.retry_hint(), RetryHint::NonRetryable);

    let aborted = HttpError::retry_aborted("policy abort");
    assert_eq!(aborted.kind, HttpErrorKind::RetryAborted);
    assert_eq!(aborted.retry_hint(), RetryHint::NonRetryable);
}

#[test]
fn test_http_error_retry_hint_status_without_status_code_is_non_retryable() {
    let error = HttpError::new(HttpErrorKind::Status, "status missing");
    assert_eq!(error.retry_hint(), RetryHint::NonRetryable);
}

#[test]
fn test_http_error_from_io_error_maps_to_transport_with_source() {
    let io_error = std::io::Error::other("disk gone");
    let error = HttpError::from(io_error);
    assert_eq!(error.kind, HttpErrorKind::Transport);
    assert!(error.message.contains("disk gone"));
    assert!(error.source.is_some());
    assert!(Error::source(&error).is_some());
}

#[test]
fn test_http_error_from_reqwest_error_maps_to_build_client_with_source() {
    let reqwest_error = reqwest::Client::new()
        .get("http://[::1")
        .build()
        .expect_err("invalid URL should fail while building request");
    let error = HttpError::from(reqwest_error);
    assert_eq!(error.kind, HttpErrorKind::BuildClient);
    assert!(error.message.contains("Failed to build reqwest client"));
    assert!(error.source.is_some());
}
