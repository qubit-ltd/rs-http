/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use qubit_config::Config;
use qubit_http::constants::DEFAULT_LOG_BODY_SIZE_LIMIT_BYTES;
use qubit_http::{HttpConfigErrorKind, HttpLoggingOptions};

#[test]
fn test_logging_options_default_values() {
    let logging = HttpLoggingOptions::default();
    assert!(logging.enabled);
    assert!(logging.log_request_header);
    assert!(logging.log_request_body);
    assert!(logging.log_response_header);
    assert!(logging.log_response_body);
    assert_eq!(logging.body_size_limit, DEFAULT_LOG_BODY_SIZE_LIMIT_BYTES);
}

#[test]
fn test_logging_options_defaults_when_no_keys() {
    let config = Config::new();
    let opts = HttpLoggingOptions::from_config(&config.prefix_view("http.logging")).unwrap();
    assert_eq!(opts, HttpLoggingOptions::default());
}

#[test]
fn test_logging_options_all_fields() {
    let mut config = Config::new();
    config.set("l.enabled", false).unwrap();
    config.set("l.log_request_header", false).unwrap();
    config.set("l.log_request_body", false).unwrap();
    config.set("l.log_response_header", false).unwrap();
    config.set("l.log_response_body", false).unwrap();
    config.set("l.body_size_limit", 4096usize).unwrap();

    let opts = HttpLoggingOptions::from_config(&config.prefix_view("l")).unwrap();
    assert!(!opts.enabled);
    assert!(!opts.log_request_header);
    assert!(!opts.log_request_body);
    assert!(!opts.log_response_header);
    assert!(!opts.log_response_body);
    assert_eq!(opts.body_size_limit, 4096);
}

#[test]
fn test_logging_validate_default_ok() {
    let opts = HttpLoggingOptions::default();
    assert!(opts.validate().is_ok());
}

#[test]
fn test_logging_validate_body_size_limit_zero_with_body_logging() {
    let opts = HttpLoggingOptions {
        log_request_body: true,
        body_size_limit: 0,
        ..Default::default()
    };
    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert!(err.path.contains("body_size_limit"));
}

#[test]
fn test_logging_validate_body_size_limit_zero_response_body() {
    let opts = HttpLoggingOptions {
        log_request_body: false,
        log_response_body: true,
        body_size_limit: 0,
        ..Default::default()
    };
    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
}

#[test]
fn test_logging_validate_body_size_limit_zero_no_body_logging_ok() {
    let opts = HttpLoggingOptions {
        log_request_body: false,
        log_response_body: false,
        body_size_limit: 0,
        ..Default::default()
    };
    assert!(opts.validate().is_ok());
}

#[test]
fn test_logging_options_from_config_can_disable_individual_flags() {
    let mut config = Config::new();
    config.set("l.enabled", false).unwrap();
    config
        .set("l.log_request_header", false)
        .expect("set boolean flag");
    config
        .set("l.log_response_body", false)
        .expect("set boolean flag");

    let opts =
        HttpLoggingOptions::from_config(&config.prefix_view("l")).expect("read logging options");
    assert!(!opts.enabled);
    assert!(!opts.log_request_header);
    assert!(!opts.log_response_body);
    assert_eq!(opts.body_size_limit, DEFAULT_LOG_BODY_SIZE_LIMIT_BYTES);
}
