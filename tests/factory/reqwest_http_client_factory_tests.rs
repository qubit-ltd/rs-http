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
fn test_factory_create_with_default_options() {
    let factory = HttpClientFactory::new();
    let options = HttpClientOptions::default();
    let client = factory.create(options).unwrap();
    assert!(!client.options().ipv4_only);
}

#[test]
fn test_factory_proxy_enabled_without_host_returns_error() {
    let factory = HttpClientFactory::new();
    let mut options = HttpClientOptions::default();
    options.proxy.enabled = true;
    options.proxy.port = Some(8080);

    let error = factory.create(options).unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::ProxyConfig);
    assert!(error.message.contains("host is missing"));
}

#[test]
fn test_factory_proxy_enabled_without_port_returns_error() {
    let factory = HttpClientFactory::new();
    let mut options = HttpClientOptions::default();
    options.proxy.enabled = true;
    options.proxy.host = Some("127.0.0.1".to_string());

    let error = factory.create(options).unwrap_err();
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

    let error = factory.create(options).unwrap_err();
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

    let client = factory.create(options).unwrap();
    assert!(!client.options().ipv4_only);
}

#[test]
fn test_factory_create_from_config_minimal() {
    let config = Config::new();
    let factory = HttpClientFactory::new();
    let client = factory.create_from_config(&config, "http").unwrap();
    assert!(!client.options().ipv4_only);
}

#[test]
fn test_factory_create_from_config_with_base_url() {
    let mut config = Config::new();
    config
        .set("http.base_url", "https://api.example.com".to_string())
        .unwrap();

    let factory = HttpClientFactory::new();
    let client = factory.create_from_config(&config, "http").unwrap();
    assert!(client.options().base_url.is_some());
}

#[test]
fn test_factory_create_from_config_proxy_validation_error() {
    let mut config = Config::new();
    config.set("http.proxy.enabled", true).unwrap();

    let factory = HttpClientFactory::new();
    let err = factory.create_from_config(&config, "http").unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::MissingField);
    assert_eq!(err.path, "http.proxy.host");
}

#[test]
fn test_factory_create_from_config_full() {
    let mut config = Config::new();
    config
        .set("svc.base_url", "https://svc.example.com".to_string())
        .unwrap();
    config.set("svc.ipv4_only", false).unwrap();
    config
        .set("svc.timeouts.connect_timeout", Duration::from_secs(3))
        .unwrap();
    config
        .set("svc.timeouts.read_timeout", Duration::from_secs(30))
        .unwrap();
    config.set("svc.logging.enabled", true).unwrap();
    config
        .set("svc.logging.body_size_limit", 4096usize)
        .unwrap();

    let factory = HttpClientFactory::new();
    let client = factory.create_from_config(&config, "svc").unwrap();
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
