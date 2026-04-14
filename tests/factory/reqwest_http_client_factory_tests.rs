/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::Duration;

use qubit_config::Config;
use qubit_http::{
    HttpClientFactory, HttpClientOptions, HttpConfigErrorKind, HttpErrorKind, ProxyType,
};

#[test]
fn test_factory_create_uses_default_options() {
    let factory = HttpClientFactory::new();
    let client = factory
        .create()
        .expect("default options should create client");

    assert!(!client.options().ipv4_only);
    assert!(!client.options().proxy.enabled);
    assert_eq!(
        client.options().timeouts.request_timeout,
        HttpClientOptions::default().timeouts.request_timeout
    );
}

#[test]
fn test_factory_create_with_options_preserves_options() {
    let factory = HttpClientFactory::new();
    let mut options = HttpClientOptions::default();
    options.timeouts.request_timeout = Some(Duration::from_secs(2));
    let client = factory
        .create_with_options(options)
        .expect("explicit options should create client");
    assert_eq!(
        client.options().timeouts.request_timeout,
        Some(Duration::from_secs(2))
    );
}

#[test]
fn test_factory_proxy_enabled_without_host_returns_error() {
    let factory = HttpClientFactory::new();
    let mut options = HttpClientOptions::default();
    options.proxy.enabled = true;
    options.proxy.port = Some(8080);

    let error = factory.create_with_options(options).unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::ProxyConfig);
    assert!(error.message.contains("host is missing"));
}

#[test]
fn test_factory_proxy_enabled_without_port_returns_error() {
    let factory = HttpClientFactory::new();
    let mut options = HttpClientOptions::default();
    options.proxy.enabled = true;
    options.proxy.host = Some("127.0.0.1".to_string());

    let error = factory.create_with_options(options).unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::ProxyConfig);
    assert!(error.message.contains("port is missing"));
}

#[test]
fn test_factory_proxy_password_without_username_returns_error() {
    let factory = HttpClientFactory::new();
    let mut options = HttpClientOptions::default();
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Http;
    options.proxy.host = Some("127.0.0.1".to_string());
    options.proxy.port = Some(8080);
    options.proxy.password = Some("secret".to_string());

    let error = factory.create_with_options(options).unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::ProxyConfig);
    assert!(error.message.contains("username is missing"));
}

#[test]
fn test_factory_proxy_with_auth_is_valid() {
    let factory = HttpClientFactory::new();
    let mut options = HttpClientOptions::default();
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Http;
    options.proxy.host = Some("127.0.0.1".to_string());
    options.proxy.port = Some(8080);
    options.proxy.username = Some("user".to_string());
    options.proxy.password = Some("pass".to_string());

    let client = factory
        .create_with_options(options)
        .expect("proxy with auth should still create client");
    assert!(!client.options().ipv4_only);
}

#[test]
fn test_factory_create_from_config_minimal() {
    let config = Config::new();
    let http = config.prefix_view("http");
    let factory = HttpClientFactory::new();
    let client = factory
        .create_from_config(&http)
        .expect("minimal config should create client");
    assert!(!client.options().ipv4_only);
}

#[test]
fn test_factory_create_from_config_with_base_url() {
    let mut config = Config::new();
    config
        .set("http.base_url", "https://api.example.com".to_string())
        .expect("test config should accept base_url");

    let factory = HttpClientFactory::new();
    let http = config.prefix_view("http");
    let client = factory
        .create_from_config(&http)
        .expect("valid config should create client");
    assert!(client.options().base_url.is_some());
}

#[test]
fn test_factory_create_from_config_scoped_reader() {
    let mut config = Config::new();
    config
        .set("http.base_url", "https://api.example.com".to_string())
        .expect("test config should accept base_url");

    let factory = HttpClientFactory::new();
    let http = config.prefix_view("http");
    let client = factory
        .create_from_config(&http)
        .expect("prefix view as ConfigReader should work");
    assert!(client.options().base_url.is_some());
}

#[test]
fn test_factory_create_from_config_root_reader_keeps_error_path() {
    let mut config = Config::new();
    config
        .set("proxy.enabled", true)
        .expect("test config should set proxy.enabled");
    config
        .set("proxy.port", 8080u16)
        .expect("test config should set proxy.port");

    let factory = HttpClientFactory::new();
    let err = factory
        .create_from_config(&config)
        .expect_err("missing proxy host should fail for root reader");
    assert_eq!(err.kind, HttpConfigErrorKind::MissingField);
    assert_eq!(err.path, "proxy.host");
}

#[test]
fn test_factory_create_from_config_proxy_validation_error() {
    let mut config = Config::new();
    config
        .set("http.proxy.enabled", true)
        .expect("test config should set proxy.enabled");

    let factory = HttpClientFactory::new();
    let http = config.prefix_view("http");
    let err = factory.create_from_config(&http).unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::MissingField);
    assert_eq!(err.path, "http.proxy.host");
}

#[test]
fn test_factory_create_from_config_root_reader_is_unchanged_when_type_error() {
    let mut config = Config::new();
    config
        .set("base_url", true)
        .expect("test config should set invalid value for base_url");

    let factory = HttpClientFactory::new();
    let err = factory
        .create_from_config(&config)
        .expect_err("invalid base_url type should fail");
    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(err.path, "base_url");
}

#[test]
fn test_factory_create_from_config_full() {
    let mut config = Config::new();
    config
        .set("svc.base_url", "https://svc.example.com".to_string())
        .expect("test config should set base_url");
    config
        .set("svc.ipv4_only", false)
        .expect("test config should set ipv4_only");
    config
        .set("svc.timeouts.connect_timeout", Duration::from_secs(3))
        .expect("test config should set connect_timeout");
    config
        .set("svc.timeouts.read_timeout", Duration::from_secs(30))
        .expect("test config should set read_timeout");
    config
        .set("svc.logging.enabled", true)
        .expect("test config should set logging.enabled");
    config
        .set("svc.logging.body_size_limit", 4096usize)
        .expect("test config should set logging.body_size_limit");

    let factory = HttpClientFactory::new();
    let svc = config.prefix_view("svc");
    let client = factory
        .create_from_config(&svc)
        .expect("full config should create client");
    assert_eq!(
        client.options().timeouts.connect_timeout,
        Duration::from_secs(3)
    );
    assert_eq!(
        client.options().timeouts.read_timeout,
        Duration::from_secs(30)
    );
    assert_eq!(client.options().logging.body_size_limit, 4096);
}

#[test]
fn test_factory_create_with_options_rejects_zero_proxy_port() {
    let mut options = HttpClientOptions::default();
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Http;
    options.proxy.host = Some("127.0.0.1".to_string());
    options.proxy.port = Some(0);

    let error = HttpClientFactory::new()
        .create_with_options(options)
        .expect_err("zero proxy port should fail");

    assert_eq!(error.kind, HttpErrorKind::ProxyConfig);
    assert!(error.message.contains("greater than 0"));
}

#[test]
fn test_factory_create_with_options_rejects_blank_proxy_host() {
    let mut options = HttpClientOptions::default();
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Http;
    options.proxy.host = Some("   ".to_string());
    options.proxy.port = Some(8080);

    let error = HttpClientFactory::new()
        .create_with_options(options)
        .expect_err("blank proxy host should fail");

    assert_eq!(error.kind, HttpErrorKind::ProxyConfig);
    assert!(error.message.contains("cannot be empty"));
}

#[test]
fn test_factory_create_with_options_rejects_zero_connect_timeout() {
    let mut options = HttpClientOptions::default();
    options.timeouts.connect_timeout = Duration::ZERO;

    let error = HttpClientFactory::new()
        .create_with_options(options)
        .expect_err("zero connect timeout should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("timeouts.connect_timeout"));
}

#[test]
fn test_factory_create_with_options_accepts_proxy_without_auth_and_request_timeout() {
    let mut options = HttpClientOptions::default();
    options.timeouts.request_timeout = Some(Duration::from_secs(2));
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Http;
    options.proxy.host = Some("127.0.0.1".to_string());
    options.proxy.port = Some(8080);

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .expect("valid proxy config should create client");

    assert_eq!(
        client.options().timeouts.request_timeout,
        Some(Duration::from_secs(2))
    );
}

#[test]
fn test_factory_create_with_options_rejects_invalid_proxy_url() {
    let mut options = HttpClientOptions::default();
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Http;
    options.proxy.host = Some("bad host".to_string());
    options.proxy.port = Some(8080);

    let error = HttpClientFactory::new()
        .create_with_options(options)
        .expect_err("invalid proxy host should fail");

    assert_eq!(error.kind, HttpErrorKind::ProxyConfig);
    assert!(error.message.contains("Invalid proxy URL"));
}

#[test]
fn test_factory_create_with_options_rejects_invalid_logging_options() {
    let mut options = HttpClientOptions::default();
    options.logging.body_size_limit = 0;

    let error = HttpClientFactory::new()
        .create_with_options(options)
        .expect_err("invalid logging options should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("logging.body_size_limit"));
}

#[test]
fn test_factory_create_with_options_rejects_invalid_retry_options() {
    let mut options = HttpClientOptions::default();
    options.retry.max_attempts = 0;

    let error = HttpClientFactory::new()
        .create_with_options(options)
        .expect_err("invalid retry options should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("retry.max_attempts"));
}

#[test]
fn test_factory_create_from_config_type_error_is_prefixed() {
    let mut config = Config::new();
    config
        .set("svc.timeouts.connect_timeout", true)
        .expect("test config should set invalid type");

    let error = HttpClientFactory::new()
        .create_from_config(&config.prefix_view("svc"))
        .expect_err("type mismatch should fail");

    assert_eq!(error.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(error.path, "svc.timeouts.connect_timeout");
}

#[test]
fn test_factory_create_from_config_maps_create_error_to_invalid_value() {
    let mut config = Config::new();
    config
        .set("svc.proxy.enabled", true)
        .expect("test config should set proxy.enabled");
    config
        .set("svc.proxy.host", "bad host".to_string())
        .expect("test config should set proxy.host");
    config
        .set("svc.proxy.port", 8080u16)
        .expect("test config should set proxy.port");

    let error = HttpClientFactory::new()
        .create_from_config(&config.prefix_view("svc"))
        .expect_err("invalid proxy URL should map to invalid value");

    assert_eq!(error.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(error.path, "svc");
    assert!(error.message.contains("Invalid proxy URL"));
}
