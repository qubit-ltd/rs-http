// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error as _;

use qubit_argument::NumericArgument;
use qubit_argument::OptionArgument;
use qubit_config::Config;
use qubit_config::ConfigError;
use qubit_datatype::DataType;
use qubit_http::HttpConfigError;
use qubit_http::HttpConfigErrorKind;

#[test]
fn test_http_config_error_display() {
    let e = HttpConfigError::missing("http.proxy.host", "host is missing");
    let s = e.to_string();
    assert!(s.contains("missing field"));
    assert!(s.contains("http.proxy.host"));
    assert!(s.contains("host is missing"));
}

#[test]
fn test_http_config_error_constructors() {
    let e = HttpConfigError::type_error("path.field", "bad type");
    assert_eq!(e.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(e.path, "path.field");

    let e = HttpConfigError::invalid_value("path.val", "out of range");
    assert_eq!(e.kind, HttpConfigErrorKind::InvalidValue);

    let e = HttpConfigError::invalid_header("path.hdr", "bad header");
    assert_eq!(e.kind, HttpConfigErrorKind::InvalidHeader);

    let e = HttpConfigError::config_error("path.cfg", "underlying error");
    assert_eq!(e.kind, HttpConfigErrorKind::ConfigError);
}

#[test]
fn test_http_config_error_is_std_error() {
    let e = HttpConfigError::missing("a.b", "msg");
    let _: &dyn std::error::Error = &e;
}

#[test]
fn test_http_config_error_from_argument_error() {
    let argument_error = 0_usize
        .require_positive("body_size_limit")
        .expect_err("zero must fail positive validation");

    let error = HttpConfigError::from(argument_error);

    assert_eq!(error.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(error.path, "body_size_limit");
    assert_eq!(error.message, "Argument validation failed");
    assert_eq!(
        error.to_string(),
        "[invalid value] body_size_limit: Argument validation failed",
    );
}

#[test]
fn test_http_config_error_from_missing_argument_error() {
    let argument_error = None::<usize>
        .require_some("proxy.port")
        .expect_err("missing option must fail required validation");

    let error = HttpConfigError::from(argument_error);

    assert_eq!(error.kind, HttpConfigErrorKind::MissingField);
    assert_eq!(error.path, "proxy.port");
    assert_eq!(error.message, "Required value is missing");
    assert_eq!(
        error.to_string(),
        "[missing field] proxy.port: Required value is missing",
    );
}

#[test]
fn test_http_config_error_from_config_error() {
    let mut config = Config::new();
    config
        .set("x", 42i32)
        .expect("test config should accept integer value");
    let ce = config
        .get_strict::<bool>("x")
        .expect_err("strictly reading integer as bool should fail");
    let he = HttpConfigError::from(ce);
    assert_eq!(he.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(he.path, "x");
    assert!(he
        .source()
        .and_then(|source| source.downcast_ref::<ConfigError>())
        .is_some());
}

#[test]
fn test_http_config_error_from_property_has_no_value_maps_to_type_error() {
    let error = HttpConfigError::from(qubit_config::ConfigError::PropertyHasNoValue(
        "svc.token".to_string(),
    ));

    assert_eq!(error.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(error.path, "svc.token");
}

#[test]
fn test_http_config_error_from_property_not_found_maps_to_config_error_with_path() {
    let error = HttpConfigError::from(qubit_config::ConfigError::PropertyNotFound(
        "svc.base_url".to_string(),
    ));

    assert_eq!(error.kind, HttpConfigErrorKind::ConfigError);
    assert_eq!(error.path, "svc.base_url");
}

#[test]
fn test_http_config_error_from_conversion_error_maps_to_type_error() {
    let error = HttpConfigError::from(qubit_config::ConfigError::ConversionError {
        key: "svc.timeout".to_string(),
        source_index: None,
        source: qubit_datatype::DataConversionError::invalid(
            DataType::String,
            DataType::Duration,
            qubit_datatype::InvalidValueReason::InvalidSyntax {
                expected: "a duration",
            },
        ),
    });

    assert_eq!(error.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(error.path, "svc.timeout");
}

#[test]
fn test_http_config_error_from_other_config_error_maps_to_config_error_without_path() {
    let error = HttpConfigError::from(qubit_config::ConfigError::Other("boom".to_string()));

    assert_eq!(error.kind, HttpConfigErrorKind::ConfigError);
    assert_eq!(error.path, "");
    assert!(error.message.contains("boom"));
}

#[test]
fn test_http_config_error_from_type_mismatch_maps_to_type_error() {
    let error = HttpConfigError::from(qubit_config::ConfigError::TypeMismatch {
        key: "svc.retries".to_string(),
        expected: DataType::Int32,
        actual: DataType::String,
    });

    assert_eq!(error.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(error.path, "svc.retries");
}

#[test]
fn test_http_config_error_new() {
    let error = HttpConfigError::new(
        HttpConfigErrorKind::ConfigError,
        "service.timeout",
        "value out of range",
    );

    assert_eq!(error.kind, HttpConfigErrorKind::ConfigError);
    assert_eq!(error.path, "service.timeout");
    assert_eq!(error.message, "value out of range");
}
