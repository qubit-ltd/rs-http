/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::str::FromStr;

use qubit_http::HttpConfigErrorKind;

#[test]
fn test_http_config_error_kind_display() {
    assert_eq!(HttpConfigErrorKind::MissingField.to_string(), "missing field");
    assert_eq!(HttpConfigErrorKind::TypeError.to_string(), "type error");
    assert_eq!(HttpConfigErrorKind::InvalidValue.to_string(), "invalid value");
    assert_eq!(HttpConfigErrorKind::InvalidHeader.to_string(), "invalid header");
    assert_eq!(HttpConfigErrorKind::ConfigError.to_string(), "config error");
}

#[test]
fn test_http_config_error_kind_from_str() {
    assert_eq!(
        HttpConfigErrorKind::from_str("missing field").expect("missing field"),
        HttpConfigErrorKind::MissingField
    );
    assert_eq!(
        HttpConfigErrorKind::from_str("TYPE ERROR").expect("type error"),
        HttpConfigErrorKind::TypeError
    );
    assert!(HttpConfigErrorKind::from_str("unknown").is_err());
}
