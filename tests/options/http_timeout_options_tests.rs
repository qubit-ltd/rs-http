// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use qubit_http::HttpConfigErrorKind;
use qubit_http::HttpTimeoutOptions;

/// Independent option construction preserves scope for domain failures.
#[test]
fn test_timeout_from_config_resolves_scoped_validation_errors() {
    let mut config = qubit_config::Config::new();
    config
        .set("service.timeouts.connect_timeout", "0ms")
        .expect("raw config");
    let error = HttpTimeoutOptions::from_config(&config.section("service.timeouts").expect("section"))
        .expect_err("zero timeout");
    assert_eq!(error.path, "service.timeouts.connect_timeout");
}

/// Independent option readers also enforce their complete schema.
#[test]
fn test_timeout_from_config_rejects_unknown_fields() {
    let mut config = qubit_config::Config::new();
    config.set("connect_timout", "1s").expect("raw config");
    let error = HttpTimeoutOptions::from_config(&config).expect_err("unknown field");
    assert_eq!(error.path, "connect_timout");
}

#[test]
fn test_http_timeout_options_validate_rejects_zero_read_timeout() {
    let options = HttpTimeoutOptions {
        read_timeout: Duration::ZERO,
        ..HttpTimeoutOptions::default()
    };

    let error = options.validate().expect_err("zero read timeout should be rejected");

    assert_eq!(error.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(error.path, "read_timeout");
}
