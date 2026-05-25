/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use qubit_config::Config;
use qubit_http::{
    HttpClientOptions,
    HttpConfigErrorKind,
};

#[test]
fn test_from_config_helpers_reports_invalid_header_value_path() {
    let mut config = Config::new();
    config
        .set("http.default_headers.x-bad", "line1\nline2".to_string())
        .expect("test config should accept raw string");

    let error =
        HttpClientOptions::from_config(&config.prefix_view("http")).expect_err("invalid header value should fail");

    assert_eq!(error.kind, HttpConfigErrorKind::InvalidHeader);
    assert_eq!(error.path, "http.default_headers.x-bad");
    assert!(error.message.contains("Invalid header value"));
}
