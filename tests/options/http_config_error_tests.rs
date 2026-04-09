/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use qubit_config::Config;
use qubit_http::{HttpConfigError, HttpConfigErrorKind};

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
fn test_http_config_error_from_config_error() {
    let mut config = Config::new();
    config.set("x", 42i32).unwrap();
    let ce = config.get::<bool>("x").unwrap_err();
    let he = HttpConfigError::from(ce);
    assert_eq!(he.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(he.path, "x");
}
