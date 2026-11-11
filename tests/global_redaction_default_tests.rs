// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests that standalone HTTP defaults snapshot the application policy.

use http::HeaderMap;
use http::Method;
use http::StatusCode;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpError;
use qubit_http::HttpErrorKind;
use qubit_http::HttpResponseMeta;
use qubit_redact::RedactionPolicy;
use qubit_redact::Redactor;
use qubit_redact::Sensitivity;
use url::Url;

struct ApplicationDefaultRestore(Redactor);

impl Drop for ApplicationDefaultRestore {
    fn drop(&mut self) {
        let _ = Redactor::replace_application_default(self.0.clone());
    }
}

#[test]
fn test_http_defaults_use_installed_global_policy() {
    let builder = RedactionPolicy::builder()
        .http(|http| {
            let _ = http.query().raise("tenant_secret", Sensitivity::Secret);
        })
        .expect("the test query field should be valid");
    let policy = builder.build().expect("the test policy should build");
    let previous = Redactor::replace_application_default(Redactor::new(policy.clone()));
    let _restore = ApplicationDefaultRestore(previous);
    let url = Url::parse("https://example.test/resource?tenant_secret=raw-tenant-secret")
        .expect("the test URL should be valid");

    let options = HttpClientOptions::default();
    let client = HttpClientBuilder::new()
        .options(options.clone())
        .build()
        .expect("client should use the options snapshot");
    let error = HttpError::new(HttpErrorKind::Transport, "request failed").with_url(&url);
    let metadata = HttpResponseMeta::new(StatusCode::OK, HeaderMap::new(), url, Method::GET);

    let _ = Redactor::replace_application_default(Redactor::new(RedactionPolicy::default()));
    let mut old_options = options.clone();
    let options_error = old_options
        .add_header(
            "https://example.test/?tenant_secret=raw-tenant-secret",
            "value",
        )
        .expect_err("URL-shaped header name should be rejected");
    let client_error = client
        .request(Method::GET, "/resource")
        .header(
            "https://example.test/?tenant_secret=raw-tenant-secret",
            "value",
        )
        .expect_err("URL-shaped header name should be rejected");

    assert_eq!(options.log_redaction_policy, policy);
    assert!(!format!("{error:?}").contains("raw-tenant-secret"));
    assert!(!format!("{metadata:?}").contains("raw-tenant-secret"));
    assert_eq!(options_error.log_redactor.policy(), &policy);
    assert_eq!(client_error.log_redactor.policy(), &policy);
    assert!(!format!("{options_error}").contains("raw-tenant-secret"));
    assert!(!format!("{client_error:?}").contains("raw-tenant-secret"));
}
