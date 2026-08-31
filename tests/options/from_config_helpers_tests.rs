// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_config::Config;
use qubit_http::HttpClientOptions;
use qubit_http::HttpConfigErrorKind;

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
