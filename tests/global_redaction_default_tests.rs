// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Tests that standalone HTTP defaults snapshot the application policy.

use http::HeaderMap;
use http::Method;
use http::StatusCode;
use qubit_http::HttpClientOptions;
use qubit_http::HttpError;
use qubit_http::HttpErrorKind;
use qubit_http::HttpResponseMeta;
use qubit_redact::RedactionPolicy;
use qubit_redact::Sensitivity;
use url::Url;

#[test]
fn test_http_defaults_use_installed_global_policy() {
    let mut builder = RedactionPolicy::builder();
    builder
        .http()
        .query()
        .raise("tenant_secret", Sensitivity::Secret)
        .expect("the test query field should be valid");
    let policy = builder.build().expect("the test policy should build");
    RedactionPolicy::install_global(policy.clone())
        .expect("this test process installs its default only once");
    let url = Url::parse("https://example.test/resource?tenant_secret=raw-tenant-secret")
        .expect("the test URL should be valid");

    let options = HttpClientOptions::default();
    let error = HttpError::new(HttpErrorKind::Transport, "request failed").with_url(&url);
    let metadata = HttpResponseMeta::new(StatusCode::OK, HeaderMap::new(), url, Method::GET);

    assert_eq!(options.log_redaction_policy, policy);
    assert!(!format!("{error:?}").contains("raw-tenant-secret"));
    assert!(!format!("{metadata:?}").contains("raw-tenant-secret"));
}
