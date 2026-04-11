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
    constants::{
        DEFAULT_CONNECT_TIMEOUT_SECS, DEFAULT_LOG_BODY_SIZE_LIMIT_BYTES, DEFAULT_READ_TIMEOUT_SECS,
        DEFAULT_WRITE_TIMEOUT_SECS,
    },
    HttpClientOptions, HttpConfigErrorKind, HttpErrorKind, ProxyType,
};

#[test]
fn test_http_client_options_defaults() {
    let options = HttpClientOptions::default();
    assert!(options.base_url.is_none());
    assert!(options.default_headers.is_empty());
    assert_eq!(
        options.timeouts.connect_timeout,
        Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS)
    );
    assert_eq!(
        options.timeouts.read_timeout,
        Duration::from_secs(DEFAULT_READ_TIMEOUT_SECS)
    );
    assert_eq!(
        options.timeouts.write_timeout,
        Duration::from_secs(DEFAULT_WRITE_TIMEOUT_SECS)
    );
    assert_eq!(options.timeouts.request_timeout, None);
    assert!(!options.proxy.enabled);
    assert_eq!(options.proxy.proxy_type, ProxyType::Http);
    assert!(options.logging.enabled);
    assert!(options.logging.log_request_header);
    assert!(options.logging.log_request_body);
    assert!(options.logging.log_response_header);
    assert!(options.logging.log_response_body);
    assert_eq!(
        options.logging.body_size_limit,
        DEFAULT_LOG_BODY_SIZE_LIMIT_BYTES
    );
    assert!(!options.ipv4_only);
}

#[test]
fn test_http_client_options_new_matches_default() {
    let options = HttpClientOptions::new();
    let defaults = HttpClientOptions::default();

    assert!(options.base_url.is_none());
    assert_eq!(options.default_headers, defaults.default_headers);
    assert_eq!(options.timeouts, defaults.timeouts);
    assert_eq!(options.proxy, defaults.proxy);
    assert_eq!(options.logging, defaults.logging);
    assert_eq!(options.sensitive_headers, defaults.sensitive_headers);
    assert_eq!(options.ipv4_only, defaults.ipv4_only);
}

#[test]
fn test_http_client_options_set_base_url() {
    let mut options = HttpClientOptions::new();

    options.set_base_url("https://api.example.com/v1").unwrap();

    assert_eq!(
        options.base_url.unwrap().as_str(),
        "https://api.example.com/v1"
    );
}

#[test]
fn test_http_client_options_set_base_url_invalid_url_does_not_apply() {
    let mut options = HttpClientOptions::new();
    options.set_base_url("https://api.example.com").unwrap();

    let error = options.set_base_url("not a url").unwrap_err();

    assert_eq!(error.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(
        options.base_url.unwrap().as_str(),
        "https://api.example.com/"
    );
}

#[test]
fn test_http_client_options_from_empty_config() {
    let config = Config::new();
    let opts = HttpClientOptions::from_config(&config).unwrap();
    assert!(opts.base_url.is_none());
    assert!(!opts.ipv4_only);
    assert!(opts.default_headers.is_empty());
}

#[test]
fn test_http_client_options_base_url() {
    let mut config = Config::new();
    config
        .set("base_url", "https://api.example.com".to_string())
        .unwrap();
    let opts = HttpClientOptions::from_config(&config).unwrap();
    assert_eq!(opts.base_url.unwrap().as_str(), "https://api.example.com/");
}

#[test]
fn test_http_client_options_invalid_base_url() {
    let mut config = Config::new();
    config.set("base_url", "not a url".to_string()).unwrap();
    let err = HttpClientOptions::from_config(&config).unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert!(err.path.contains("base_url"));
}

#[test]
fn test_http_client_options_ipv4_only() {
    let mut config = Config::new();
    config.set("ipv4_only", true).unwrap();
    let opts = HttpClientOptions::from_config(&config).unwrap();
    assert!(opts.ipv4_only);
}

#[test]
fn test_http_client_options_with_prefix() {
    let mut config = Config::new();
    config
        .set("http.base_url", "https://example.com".to_string())
        .unwrap();
    config.set("http.ipv4_only", true).unwrap();
    config
        .set("http.timeouts.connect_timeout", Duration::from_secs(5))
        .unwrap();

    let opts = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap();
    assert!(opts.base_url.is_some());
    assert!(opts.ipv4_only);
    assert_eq!(opts.timeouts.connect_timeout, Duration::from_secs(5));
}

#[test]
fn test_http_client_options_default_headers_subkey_form() {
    let mut config = Config::new();
    config
        .set(
            "http.default_headers.authorization",
            "Bearer token123".to_string(),
        )
        .unwrap();
    config
        .set("http.default_headers.x-request-id", "abc-123".to_string())
        .unwrap();

    let opts = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap();
    assert_eq!(opts.default_headers.len(), 2);
    assert!(opts.default_headers.contains_key("authorization"));
    assert!(opts.default_headers.contains_key("x-request-id"));
}

#[test]
fn test_http_client_options_default_headers_json_form() {
    let mut config = Config::new();
    config
        .set(
            "http.default_headers",
            r#"{"x-app-id":"my-app","x-version":"1.0"}"#.to_string(),
        )
        .unwrap();

    let opts = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap();
    assert_eq!(opts.default_headers.len(), 2);
}

#[test]
fn test_http_client_options_default_headers_invalid_json() {
    let mut config = Config::new();
    config
        .set("http.default_headers", "not-json".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
}

#[test]
fn test_http_client_options_invalid_header_name() {
    let mut config = Config::new();
    config
        .set("http.default_headers.invalid header", "value".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidHeader);
}

#[test]
fn test_http_client_options_invalid_header_value_from_config() {
    let mut config = Config::new();
    config
        .set("http.default_headers.x-bad", "line1\nline2".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::InvalidHeader);
}

#[test]
fn test_http_client_options_non_string_header_value_from_config() {
    let mut config = Config::new();
    config.set("http.default_headers.x-number", 42_i32).unwrap();

    let err = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::ConfigError);
    assert!(err.path.contains("default_headers.x-number"));
}

#[test]
fn test_http_client_options_add_header_and_add_headers_are_atomic() {
    let mut opts = HttpClientOptions::new();
    opts.add_header("x-app", "qubit").unwrap();
    opts.add_headers([("x-env", "test"), ("x-region", "cn")])
        .unwrap();

    assert_eq!(opts.default_headers.get("x-app").unwrap(), "qubit");
    assert_eq!(opts.default_headers.get("x-env").unwrap(), "test");
    assert_eq!(opts.default_headers.get("x-region").unwrap(), "cn");

    let error = opts
        .add_headers([("x-keep", "value"), ("bad header", "boom")])
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(!opts.default_headers.contains_key("x-keep"));
}

#[test]
fn test_http_client_options_add_header_invalid_value_does_not_apply() {
    let mut opts = HttpClientOptions::new();

    let error = opts.add_header("x-bad", "line1\nline2").unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(!opts.default_headers.contains_key("x-bad"));
}

#[test]
fn test_http_client_options_sensitive_headers() {
    let mut config = Config::new();
    config
        .set(
            "http.sensitive_headers",
            vec!["X-Custom-Secret".to_string(), "X-Api-Token".to_string()],
        )
        .unwrap();

    let opts = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap();
    assert!(opts.sensitive_headers.contains("x-custom-secret"));
    assert!(opts.sensitive_headers.contains("x-api-token"));
}

#[test]
fn test_http_client_options_proxy_section() {
    let mut config = Config::new();
    config.set("http.proxy.enabled", true).unwrap();
    config
        .set("http.proxy.host", "proxy.corp.example.com".to_string())
        .unwrap();
    config.set("http.proxy.port", 3128u16).unwrap();

    let opts = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap();
    assert!(opts.proxy.enabled);
    assert_eq!(opts.proxy.host, Some("proxy.corp.example.com".to_string()));
    assert_eq!(opts.proxy.port, Some(3128));
}

#[test]
fn test_http_client_options_proxy_section_invalid_type_is_prefixed() {
    let mut config = Config::new();
    config
        .set("http.proxy.proxy_type", "bad-proxy".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "proxy.proxy_type");
}

#[test]
fn test_http_client_options_logging_section() {
    let mut config = Config::new();
    config.set("http.logging.enabled", false).unwrap();
    config
        .set("http.logging.body_size_limit", 8192usize)
        .unwrap();

    let opts = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap();
    assert!(!opts.logging.enabled);
    assert_eq!(opts.logging.body_size_limit, 8192);
}

#[test]
fn test_http_client_options_logging_section_type_error_is_prefixed() {
    let mut config = Config::new();
    config
        .set("http.logging.enabled", "not-bool".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(err.path, "logging.enabled");
}

#[test]
fn test_http_client_options_validate_default_ok() {
    let opts = HttpClientOptions::default();
    assert!(opts.validate().is_ok());
}

#[test]
fn test_http_client_options_validate_propagates_proxy_error() {
    let mut opts = HttpClientOptions::default();
    opts.proxy.enabled = true;

    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::MissingField);
}

#[test]
fn test_http_client_options_validate_propagates_logging_error() {
    let mut opts = HttpClientOptions::default();
    opts.logging.log_request_body = true;
    opts.logging.body_size_limit = 0;

    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
}

#[test]
fn test_from_config_empty_prefix() {
    let mut config = Config::new();
    config
        .set("base_url", "https://root.example.com".to_string())
        .unwrap();
    config.set("ipv4_only", true).unwrap();

    let opts = HttpClientOptions::from_config(&config).unwrap();
    assert!(opts.base_url.is_some());
    assert!(opts.ipv4_only);
}
