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
    let opts = HttpTimeoutOptions::from_config(&config.section("http.timeouts").unwrap()).unwrap();
    assert_eq!(opts, HttpTimeoutOptions::default());
}

#[test]
fn test_timeout_options_all_fields() {
    let mut config = Config::new();
    config
        .set("http.timeouts.connect_timeout", Duration::from_secs(5))
        .unwrap();
    config
        .set("http.timeouts.read_timeout", Duration::from_secs(30))
        .unwrap();
    config
        .set("http.timeouts.write_timeout", Duration::from_secs(20))
        .unwrap();
    config
        .set("http.timeouts.request_timeout", Duration::from_secs(60))
        .unwrap();

    let opts = HttpTimeoutOptions::from_config(&config.section("http.timeouts").unwrap()).unwrap();
    assert_eq!(opts.connect_timeout, Duration::from_secs(5));
    assert_eq!(opts.read_timeout, Duration::from_secs(30));
    assert_eq!(opts.write_timeout, Duration::from_secs(20));
    assert_eq!(opts.request_timeout, Some(Duration::from_secs(60)));
}

#[test]
fn test_timeout_options_partial_fields() {
    let mut config = Config::new();
    config.set("t.connect_timeout", Duration::from_millis(500)).unwrap();

    let opts = HttpTimeoutOptions::from_config(&config.section("t").unwrap()).unwrap();
    assert_eq!(opts.connect_timeout, Duration::from_millis(500));
    assert_eq!(opts.read_timeout, Duration::from_secs(120));
    assert_eq!(opts.request_timeout, None);
}

#[test]
fn test_timeout_options_no_request_timeout() {
    let mut config = Config::new();
    config.set("t.connect_timeout", Duration::from_secs(10)).unwrap();

    let opts = HttpTimeoutOptions::from_config(&config.section("t").unwrap()).unwrap();
    assert_eq!(opts.request_timeout, None);
}

#[test]
fn test_timeout_options_invalid_type_is_prefixed() {
    let mut config = Config::new();
    config.set("t.connect_timeout", "invalid").unwrap();

    let err = HttpTimeoutOptions::from_config(&config.section("t").unwrap()).unwrap_err();

    assert_eq!(err.path, "t.connect_timeout");
}

#[test]
fn test_timeout_options_invalid_read_timeout_type_is_prefixed() {
    let mut config = Config::new();
    config.set("t.read_timeout", "invalid").unwrap();

    let err = HttpTimeoutOptions::from_config(&config.section("t").unwrap()).unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(err.path, "t.read_timeout");
}

#[test]
fn test_timeout_options_invalid_write_timeout_type_is_prefixed() {
    let mut config = Config::new();
    config.set("t.write_timeout", "invalid").unwrap();

    let err = HttpTimeoutOptions::from_config(&config.section("t").unwrap()).unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(err.path, "t.write_timeout");
}

#[test]
fn test_timeout_options_validate_rejects_zero_values() {
    let mut opts = HttpTimeoutOptions::default();
    opts.connect_timeout = Duration::ZERO;

    let err = opts.validate().unwrap_err();
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

    let err = HttpTimeoutOptions::from_config(&config.section("t").unwrap()).unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "t.request_timeout");
}
