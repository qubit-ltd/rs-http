// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_config::Config;
use qubit_config::ConfigError;
use qubit_http::HttpClientOptions;
use qubit_http::HttpConfigErrorKind;

/// Header conversion failures retain the original structured config error.
#[test]
fn test_from_config_preserves_header_read_error_source() {
    let mut config = Config::new();
    config
        .set(
            "service.default_headers.x-values",
            "${service.default_headers.x-values}",
        )
        .expect("cycle");
    let error = HttpClientOptions::from_config(&config.section("service").expect("section"))
        .expect_err("header interpolation cycle");
    assert_eq!(error.path, "service.default_headers.x-values");
    let source = std::error::Error::source(&error)
        .expect("original config error")
        .downcast_ref::<ConfigError>()
        .expect("structured config source");
    assert_eq!(source.path(), Some("service.default_headers.x-values"));
}

/// Root-only domain constraints are enforced before options are returned.
#[test]
fn test_from_config_validates_root_user_agent() {
    for value in [" ", "agent\ninvalid"] {
        let mut config = Config::new();
        config.set("service.user_agent", value).expect("raw config");
        let error = HttpClientOptions::from_config(&config.section("service").expect("section"))
            .expect_err("invalid user agent");
        assert_eq!(error.path, "service.user_agent");
    }
}

/// Component readers reject misspellings while preserving their full scope.
#[test]
fn test_from_config_rejects_unknown_owned_keys() {
    for key in [
        "connect_timout",
        "timeouts.connect_timout",
        "json.max_node",
        "sse.max_frame_byte",
        "log_redaction.sensitive_header",
        "timeouts",
    ] {
        let mut config = Config::new();
        let path = format!("service.{key}");
        config.set(&path, "1").expect("configuration accepts arbitrary keys");
        let error = HttpClientOptions::from_config(&config.section("service").expect("section"))
            .expect_err("unknown HTTP key must be rejected");
        assert_eq!(error.path, path);
        let source = std::error::Error::source(&error)
            .expect("structured configuration error")
            .downcast_ref::<qubit_config::ConfigError>()
            .expect("config source");
        assert_eq!(source.kind(), qubit_config::ConfigErrorKind::UnknownProperty);
    }
}

/// An unrelated sibling is outside the selected HTTP section.
#[test]
fn test_from_config_keeps_open_headers_and_sibling_sections() {
    let mut config = Config::new();
    config.set("other.option", true).expect("sibling");
    config
        .set("service.default_headers.x-custom.header", "value")
        .expect("header");
    let options = HttpClientOptions::from_config(&config.section("service").expect("section"))
        .expect("open header names and unrelated siblings are allowed");
    assert_eq!(options.default_headers["x-custom.header"], "value");
}

/// A scope identical to a field name must not suppress prefix resolution.
#[test]
fn test_from_config_resolves_domain_paths_without_prefix_guessing() {
    let mut config = Config::new();
    config.set("base_url.base_url", "invalid URL").expect("raw config");
    let error = HttpClientOptions::from_config(&config.section("base_url").expect("section")).expect_err("invalid URL");
    assert_eq!(error.path, "base_url.base_url");
}

/// Every option constructor returns domain-valid options with scoped failures.
#[test]
fn test_from_config_validates_proxy_and_logging_at_their_scope() {
    let mut config = Config::new();
    config.set("service.proxy.enabled", true).expect("enabled proxy");
    let error = qubit_http::ProxyOptions::from_config(&config.section("service.proxy").expect("section"))
        .expect_err("enabled proxy requires host");
    assert_eq!(error.path, "service.proxy.host");
    config.set("service.logging.body_size_limit", 0u64).expect("zero limit");
    let error = qubit_http::HttpLoggingOptions::from_config(&config.section("service.logging").expect("section"))
        .expect_err("body logging requires positive limit");
    assert_eq!(error.path, "service.logging.body_size_limit");
}

#[test]
fn test_from_config_helpers_reports_invalid_header_value_path() {
    let mut config = Config::new();
    config
        .set("http.default_headers.x-bad", "line1\nline2".to_string())
        .expect("test config should accept raw string");

    let error =
        HttpClientOptions::from_config(&config.section("http").unwrap()).expect_err("invalid header value should fail");

    assert_eq!(error.kind, HttpConfigErrorKind::InvalidHeader);
    assert_eq!(error.path, "http.default_headers.x-bad");
    assert!(error.message.contains("Invalid header value"));
}

#[cfg(target_pointer_width = "64")]
#[test]
fn test_from_config_helpers_accepts_u64_max_as_usize() {
    let mut config = Config::new();
    config
        .set("http.max_redirects", u64::MAX)
        .expect("test config should accept u64::MAX");

    let options = HttpClientOptions::from_config(&config.section("http").unwrap())
        .expect("u64::MAX should fit in a 64-bit usize");

    assert_eq!(options.max_redirects, Some(usize::MAX));
}

#[cfg(target_pointer_width = "32")]
#[test]
fn test_from_config_helpers_rejects_usize_overflow() {
    let mut config = Config::new();
    config
        .set("http.max_redirects", u64::from(u32::MAX) + 1)
        .expect("test config should accept the platform-independent u64");

    let error = HttpClientOptions::from_config(&config.section("http").unwrap())
        .expect_err("a value larger than usize should fail");

    assert_eq!(error.kind, HttpConfigErrorKind::ConfigError);
    assert_eq!(error.path, "http.max_redirects");
    assert!(
        error
            .message
            .contains("configuration value exceeds the platform usize range")
    );
}
