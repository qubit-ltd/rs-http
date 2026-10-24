// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use qubit_config::Config;
use qubit_http::HttpConfigErrorKind;
use qubit_http::HttpTimeoutOptions;

#[test]
fn test_timeout_options_defaults_when_no_keys() {
    let config = Config::new();
    let section = config.section("http.timeouts").expect("timeout section should exist");
    let opts = HttpTimeoutOptions::from_config(&section).expect("default timeout options should be valid");
    assert_eq!(opts, HttpTimeoutOptions::default());
}

#[test]
fn test_timeout_options_all_fields() {
    let mut config = Config::new();
    config
        .set("http.timeouts.connect_timeout", Duration::from_secs(5))
        .expect("connect timeout should be configurable");
    config
        .set("http.timeouts.read_timeout", Duration::from_secs(30))
        .expect("read timeout should be configurable");
    config
        .set("http.timeouts.response_header_timeout", Duration::from_secs(20))
        .expect("response header timeout should be configurable");
    config
        .set("http.timeouts.request_timeout", Duration::from_secs(60))
        .expect("request timeout should be configurable");

    let section = config.section("http.timeouts").expect("timeout section should exist");
    let opts = HttpTimeoutOptions::from_config(&section).expect("all timeout options should be valid");
    assert_eq!(opts.connect_timeout, Duration::from_secs(5));
    assert_eq!(opts.read_timeout, Duration::from_secs(30));
    assert_eq!(opts.response_header_timeout, Duration::from_secs(20));
    assert_eq!(opts.request_timeout, Some(Duration::from_secs(60)));
}

#[test]
fn test_timeout_options_partial_fields() {
    let mut config = Config::new();
    config
        .set("t.connect_timeout", Duration::from_millis(500))
        .expect("connect timeout should be configurable");

    let section = config.section("t").expect("timeout section should exist");
    let opts = HttpTimeoutOptions::from_config(&section).expect("partial timeout options should be valid");
    assert_eq!(opts.connect_timeout, Duration::from_millis(500));
    assert_eq!(opts.read_timeout, Duration::from_secs(120));
    assert_eq!(opts.request_timeout, None);
}

#[test]
fn test_timeout_options_no_request_timeout() {
    let mut config = Config::new();
    config
        .set("t.connect_timeout", Duration::from_secs(10))
        .expect("connect timeout should be configurable");

    let section = config.section("t").expect("timeout section should exist");
    let opts =
        HttpTimeoutOptions::from_config(&section).expect("timeout options without a request timeout should be valid");
    assert_eq!(opts.request_timeout, None);
}

#[test]
fn test_timeout_options_invalid_type_is_prefixed() {
    let mut config = Config::new();
    config
        .set("t.connect_timeout", "invalid")
        .expect("invalid timeout value should be storable as raw config");

    let section = config.section("t").expect("timeout section should exist");
    let err = HttpTimeoutOptions::from_config(&section).expect_err("invalid connect timeout should be rejected");

    assert_eq!(err.path, "t.connect_timeout");
}

#[test]
fn test_timeout_options_invalid_read_timeout_type_is_prefixed() {
    let mut config = Config::new();
    config
        .set("t.read_timeout", "invalid")
        .expect("invalid timeout value should be storable as raw config");

    let section = config.section("t").expect("timeout section should exist");
    let err = HttpTimeoutOptions::from_config(&section).expect_err("invalid read timeout should be rejected");

    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(err.path, "t.read_timeout");
}

#[test]
fn test_timeout_options_invalid_response_header_timeout_type_is_prefixed() {
    let mut config = Config::new();
    config
        .set("t.response_header_timeout", "invalid")
        .expect("invalid timeout value should be storable as raw config");

    let section = config.section("t").expect("timeout section should exist");
    let err =
        HttpTimeoutOptions::from_config(&section).expect_err("invalid response header timeout should be rejected");

    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(err.path, "t.response_header_timeout");
}

#[test]
fn test_timeout_options_validate_rejects_zero_values() {
    let mut opts = HttpTimeoutOptions::default();
    opts.connect_timeout = Duration::ZERO;

    let err = opts.validate().expect_err("zero connect timeout should be rejected");
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "connect_timeout");
    assert_eq!(err.message, "Timeout value must be greater than 0");
    assert_eq!(
        err.to_string(),
        "[invalid value] connect_timeout: Timeout value must be greater than 0",
    );
}

#[test]
fn test_timeout_options_from_config_rejects_zero_request_timeout() {
    let mut config = Config::new();
    config
        .set("t.request_timeout", Duration::ZERO)
        .expect("test config should set request_timeout");

    let section = config.section("t").expect("timeout section should exist");
    let err = HttpTimeoutOptions::from_config(&section).expect_err("zero request timeout should be rejected");
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "t.request_timeout");
}
