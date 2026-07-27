// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_config::Config;
use qubit_http::{HttpConfigErrorKind, ProxyOptions, ProxyType};

#[test]
fn test_proxy_options_default_values() {
    let proxy = ProxyOptions::default();
    assert!(!proxy.enabled);
    assert_eq!(proxy.proxy_type, ProxyType::Http);
    assert!(proxy.host.is_none());
    assert!(proxy.port.is_none());
    assert!(proxy.username.is_none());
    assert!(proxy.password.is_none());
}

#[test]
fn test_proxy_options_defaults_when_no_keys() {
    let config = Config::new();
    let opts = ProxyOptions::from_config(&config.section("http.proxy")).unwrap();
    assert_eq!(opts, ProxyOptions::default());
}

#[test]
fn test_proxy_options_http_type() {
    let mut config = Config::new();
    config.set("p.enabled", true).unwrap();
    config.set("p.proxy_type", "http".to_string()).unwrap();
    config.set("p.host", "127.0.0.1".to_string()).unwrap();
    config.set("p.port", 8080u16).unwrap();

    let opts = ProxyOptions::from_config(&config.section("p")).unwrap();
    assert!(opts.enabled);
    assert_eq!(opts.proxy_type, ProxyType::Http);
    assert_eq!(opts.host, Some("127.0.0.1".to_string()));
    assert_eq!(opts.port, Some(8080));
}

#[test]
fn test_proxy_options_https_type() {
    let mut config = Config::new();
    config.set("p.proxy_type", "https".to_string()).unwrap();
    let opts = ProxyOptions::from_config(&config.section("p")).unwrap();
    assert_eq!(opts.proxy_type, ProxyType::Https);
}

#[test]
fn test_proxy_options_socks5_type() {
    let mut config = Config::new();
    config.set("p.proxy_type", "socks5".to_string()).unwrap();
    let opts = ProxyOptions::from_config(&config.section("p")).unwrap();
    assert_eq!(opts.proxy_type, ProxyType::Socks5);
}

#[test]
fn test_proxy_options_socks5h_type() {
    let mut config = Config::new();
    config.set("p.proxy_type", "socks5h".to_string()).unwrap();
    let opts = ProxyOptions::from_config(&config.section("p")).unwrap();
    assert_eq!(opts.proxy_type, ProxyType::Socks5);
}

#[test]
fn test_proxy_options_proxy_type_case_insensitive() {
    let mut config = Config::new();
    config.set("p.proxy_type", "HtTp".to_string()).unwrap();
    let opts = ProxyOptions::from_config(&config.section("p")).unwrap();
    assert_eq!(opts.proxy_type, ProxyType::Http);
}

#[test]
fn test_proxy_options_unknown_type_returns_error() {
    let mut config = Config::new();
    config.set("p.proxy_type", "ftp".to_string()).unwrap();
    let err = ProxyOptions::from_config(&config.section("p")).unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert!(err.message.contains("ftp"));
}

#[test]
fn test_proxy_options_invalid_enabled_type_is_prefixed() {
    let mut config = Config::new();
    config.set("p.enabled", "not-bool").unwrap();

    let err = ProxyOptions::from_config(&config.section("p")).unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(err.path, "p.enabled");
}

#[test]
fn test_proxy_options_invalid_port_type_is_prefixed() {
    let mut config = Config::new();
    config.set("p.port", "not-port").unwrap();

    let err = ProxyOptions::from_config(&config.section("p")).unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(err.path, "p.port");
}

#[test]
fn test_proxy_options_with_auth() {
    let mut config = Config::new();
    config.set("p.enabled", true).unwrap();
    config
        .set("p.host", "proxy.example.com".to_string())
        .unwrap();
    config.set("p.port", 3128u16).unwrap();
    config.set("p.username", "user".to_string()).unwrap();
    config.set("p.password", "pass".to_string()).unwrap();

    let opts = ProxyOptions::from_config(&config.section("p")).unwrap();
    assert_eq!(opts.username, Some("user".to_string()));
    assert_eq!(opts.password, Some("pass".to_string()));
}

#[test]
fn test_proxy_validate_disabled_always_ok() {
    let opts = ProxyOptions::default();
    assert!(opts.validate().is_ok());
}

#[test]
fn test_proxy_validate_enabled_missing_host() {
    let opts = ProxyOptions {
        enabled: true,
        port: Some(8080),
        ..Default::default()
    };
    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::MissingField);
    assert!(err.path.contains("host"));
}

#[test]
fn test_proxy_validate_enabled_missing_port() {
    let opts = ProxyOptions {
        enabled: true,
        host: Some("127.0.0.1".to_string()),
        ..Default::default()
    };
    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::MissingField);
    assert!(err.path.contains("port"));
}

#[test]
fn test_proxy_validate_enabled_port_zero() {
    let opts = ProxyOptions {
        enabled: true,
        host: Some("127.0.0.1".to_string()),
        port: Some(0),
        ..Default::default()
    };
    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert!(err.message.contains("greater than 0"));
}

#[test]
fn test_proxy_validate_password_without_username() {
    let opts = ProxyOptions {
        password: Some("secret".to_string()),
        ..Default::default()
    };
    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::MissingField);
    assert!(err.path.contains("username"));
}

#[test]
fn test_proxy_validate_enabled_blank_host() {
    let opts = ProxyOptions {
        enabled: true,
        host: Some("   ".to_string()),
        port: Some(8080),
        ..Default::default()
    };

    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "proxy.host");
}

#[test]
fn test_proxy_validate_rejects_blank_username() {
    let opts = ProxyOptions {
        username: Some("  ".to_string()),
        ..Default::default()
    };

    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "proxy.username");
}

#[test]
fn test_proxy_validate_full_valid_config() {
    let opts = ProxyOptions {
        enabled: true,
        host: Some("proxy.example.com".to_string()),
        port: Some(8080),
        username: Some("user".to_string()),
        password: Some("pass".to_string()),
        ..Default::default()
    };
    assert!(opts.validate().is_ok());
}

#[test]
fn test_proxy_options_no_auth() {
    let mut config = Config::new();
    config.set("p.enabled", true).unwrap();
    config
        .set("p.host", "proxy.example.com".to_string())
        .unwrap();
    config.set("p.port", 8080u16).unwrap();

    let opts = ProxyOptions::from_config(&config.section("p")).unwrap();
    assert!(opts.username.is_none());
    assert!(opts.password.is_none());
    assert!(opts.validate().is_ok());
}
