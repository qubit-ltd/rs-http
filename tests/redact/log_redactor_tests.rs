// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use http::{
    HeaderMap,
    HeaderValue,
};
use qubit_http::{
    LogRedactionPolicy,
    LogRedactor,
};
use qubit_redact::http::{
    BodyBudget,
    UrlPathPolicy,
};
use url::Url;

/// Verifies that presentation and hard budgets both constrain body previews.
#[test]
fn test_body_preview_obeys_both_presentation_and_hard_budget() {
    let policy = LogRedactionPolicy::builder_from_default()
        .body_budget(BodyBudget::new(32, 48).expect("budget should be valid"))
        .build()
        .expect("policy should be valid");
    let redactor = LogRedactor::new(policy);
    let body = br#"{"password":"never-log-this","padding":"xxxxxxxxxxxxxxxxxxxxxxxx"}"#;

    let rendered =
        redactor.redact_body_preview(body, 24, Some("application/json"));

    assert!(!rendered.to_string().contains("never-log-this"));
    assert!(rendered.is_truncated());
    assert_eq!(rendered.source_len(), Some(body.len()));
    assert_eq!(rendered.omitted_len(), Some(body.len() - 24));
}

/// Verifies that the runtime hard limit still applies above presentation.
#[test]
fn test_body_preview_hard_budget_preserves_exact_metadata() {
    let policy = LogRedactionPolicy::builder_from_default()
        .body_budget(BodyBudget::new(8, 64).expect("budget should be valid"))
        .build()
        .expect("policy should be valid");
    let redactor = LogRedactor::new(policy);
    let body = b"abcdefghijklmnop";

    let rendered = redactor.redact_body_preview(body, 12, None);

    assert_eq!(rendered.captured_len(), 8);
    assert_eq!(rendered.source_len(), Some(body.len()));
    assert_eq!(rendered.omitted_len(), Some(body.len() - 8));
    assert!(rendered.is_truncated());
}

/// Verifies that the public preview API clamps zero to one captured byte.
#[test]
fn test_body_preview_zero_limit_preserves_truthful_metadata() {
    let redactor = LogRedactor::default();
    let body = b"zero-limit-secret";

    let rendered = redactor.redact_body_preview(body, 0, None);

    assert_eq!(rendered.captured_len(), 1);
    assert_eq!(rendered.source_len(), Some(body.len()));
    assert_eq!(rendered.omitted_len(), Some(body.len() - 1));
    assert!(rendered.is_truncated());
    assert!(!rendered.to_string().contains("zero-limit-secret"));
}

/// Verifies that URL, headers, and body all use the same policy snapshot.
#[test]
fn test_log_redactor_uses_one_policy_snapshot() {
    let policy = LogRedactionPolicy::builder_from_default()
        .allow_query_exact("public_token")
        .allow_header_exact("x-public-token")
        .allow_body_exact("public_token")
        .url_path_policy(UrlPathPolicy::Preserve)
        .build()
        .expect("policy should be valid");
    let redactor = LogRedactor::new(policy);
    let url =
        Url::parse("https://example.com/private/path?public_token=query-value")
            .expect("URL should parse");
    let mut headers = HeaderMap::new();
    headers.insert("x-public-token", HeaderValue::from_static("header-value"));

    assert!(redactor
        .redact_url(&url)
        .to_string()
        .contains("private/path"));
    assert!(redactor
        .redact_url(&url)
        .to_string()
        .contains("query-value"));
    assert!(redactor
        .redact_headers(&headers)
        .to_string()
        .contains("header-value"));
    assert!(redactor
        .redact_body_preview(
            br#"{"public_token":"body-value"}"#,
            1024,
            Some("application/json"),
        )
        .to_string()
        .contains("body-value"),);
}

/// Verifies default URL, native-header, and structured-body secret absence.
#[test]
fn test_log_redactor_default_never_exposes_cross_domain_sentinels() {
    let redactor = LogRedactor::default();
    let url = Url::parse(
        "https://url-user:url-password@example.com/private/path?access_token=query-secret#fragment-secret",
    )
    .expect("URL should parse");
    let mut headers = HeaderMap::new();
    let mut native_secret = HeaderValue::from_static("native-header-secret");
    native_secret.set_sensitive(true);
    headers.insert("x-diagnostic", native_secret);
    let body =
        br#"{"password":"body-password","items":[{"token":"body-token"}]}"#;

    let url_text = redactor.redact_url(&url).to_string();
    let header_text = redactor.redact_headers(&headers).to_string();
    let body_text = redactor
        .redact_body_preview(body, body.len(), Some("application/json"))
        .to_string();

    for sentinel in [
        "url-user",
        "url-password",
        "query-secret",
        "fragment-secret",
    ] {
        assert!(!url_text.contains(sentinel));
    }
    assert!(!header_text.contains("native-header-secret"));
    assert!(!body_text.contains("body-password"));
    assert!(!body_text.contains("body-token"));
}

/// Verifies invalid Content-Type text fails closed without exposing body data.
#[test]
fn test_log_redactor_invalid_content_type_fails_closed() {
    let redactor = LogRedactor::default();
    let body = b"invalid-content-type-secret";

    let rendered =
        redactor.redact_body_preview(body, body.len(), Some("bad\nvalue"));

    assert!(!rendered.to_string().contains("invalid-content-type-secret"));
}
