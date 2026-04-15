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
        DEFAULT_CONNECT_TIMEOUT_SECS, DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES,
        DEFAULT_LOG_BODY_SIZE_LIMIT_BYTES, DEFAULT_READ_TIMEOUT_SECS, DEFAULT_SSE_MAX_FRAME_BYTES,
        DEFAULT_SSE_MAX_LINE_BYTES, DEFAULT_WRITE_TIMEOUT_SECS,
    },
    sse::{DoneMarkerPolicy, SseJsonMode},
    HttpClientOptions, HttpConfigErrorKind, HttpErrorKind, HttpRetryMethodPolicy,
    HttpRetryOptions, ProxyType, RetryDelay,
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
    assert_eq!(
        options.error_response_preview_limit,
        DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES
    );
    assert_eq!(options.user_agent, None);
    assert_eq!(options.max_redirects, None);
    assert_eq!(options.pool_idle_timeout, None);
    assert_eq!(options.pool_max_idle_per_host, None);
    assert_eq!(options.retry, HttpRetryOptions::default());
    assert!(!options.retry.enabled);
    assert_eq!(options.retry.max_attempts, 3);
    assert_eq!(
        options.retry.method_policy,
        HttpRetryMethodPolicy::IdempotentOnly
    );
    assert!(!options.ipv4_only);
    assert_eq!(options.sse_json_mode, SseJsonMode::Lenient);
    assert_eq!(options.sse_done_marker_policy, DoneMarkerPolicy::default());
    assert_eq!(options.sse_max_line_bytes, DEFAULT_SSE_MAX_LINE_BYTES);
    assert_eq!(options.sse_max_frame_bytes, DEFAULT_SSE_MAX_FRAME_BYTES);
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
    assert_eq!(
        options.error_response_preview_limit,
        defaults.error_response_preview_limit
    );
    assert_eq!(options.user_agent, defaults.user_agent);
    assert_eq!(options.max_redirects, defaults.max_redirects);
    assert_eq!(options.pool_idle_timeout, defaults.pool_idle_timeout);
    assert_eq!(
        options.pool_max_idle_per_host,
        defaults.pool_max_idle_per_host
    );
    assert_eq!(options.retry, defaults.retry);
    assert_eq!(options.sensitive_headers, defaults.sensitive_headers);
    assert_eq!(options.ipv4_only, defaults.ipv4_only);
    assert_eq!(options.sse_json_mode, defaults.sse_json_mode);
    assert_eq!(
        options.sse_done_marker_policy,
        defaults.sse_done_marker_policy
    );
    assert_eq!(options.sse_max_line_bytes, defaults.sse_max_line_bytes);
    assert_eq!(options.sse_max_frame_bytes, defaults.sse_max_frame_bytes);
}

#[test]
fn test_http_retry_options_new_matches_default() {
    assert_eq!(HttpRetryOptions::new(), HttpRetryOptions::default());
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
fn test_http_client_options_reqwest_extra_fields_from_config() {
    let mut config = Config::new();
    config
        .set("http.user_agent", "qubit-http-tests/1.0".to_string())
        .unwrap();
    config.set("http.max_redirects", 7_usize).unwrap();
    config
        .set("http.pool_idle_timeout", Duration::from_secs(15))
        .unwrap();
    config.set("http.pool_max_idle_per_host", 32_usize).unwrap();

    let opts = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap();
    assert_eq!(opts.user_agent.as_deref(), Some("qubit-http-tests/1.0"));
    assert_eq!(opts.max_redirects, Some(7));
    assert_eq!(opts.pool_idle_timeout, Some(Duration::from_secs(15)));
    assert_eq!(opts.pool_max_idle_per_host, Some(32));
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
    assert_eq!(err.path, "http.proxy.proxy_type");
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
fn test_http_client_options_error_response_preview_limit_from_config() {
    let mut config = Config::new();
    config
        .set("http.error_response_preview_limit", 512usize)
        .expect("test config should set error_response_preview_limit");

    let opts = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap();
    assert_eq!(opts.error_response_preview_limit, 512);
}

#[test]
fn test_http_client_options_logging_section_type_error_is_prefixed() {
    let mut config = Config::new();
    config
        .set("http.logging.enabled", "not-bool".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(err.path, "http.logging.enabled");
}

#[test]
fn test_http_client_options_retry_section() {
    let mut config = Config::new();
    config.set("http.retry.enabled", true).unwrap();
    config.set("http.retry.max_attempts", 4_u32).unwrap();
    config
        .set("http.retry.max_duration", Duration::from_secs(30))
        .unwrap();
    config
        .set(
            "http.retry.delay_strategy",
            "EXPONENTIAL_BACKOFF".to_string(),
        )
        .unwrap();
    config
        .set(
            "http.retry.backoff_initial_delay",
            Duration::from_millis(50),
        )
        .unwrap();
    config
        .set("http.retry.backoff_max_delay", Duration::from_secs(2))
        .unwrap();
    config
        .set("http.retry.backoff_multiplier", 1.5_f64)
        .unwrap();
    config.set("http.retry.jitter_factor", 0.25_f64).unwrap();
    config
        .set("http.retry.method_policy", "ALL_METHODS".to_string())
        .unwrap();

    let opts = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap();

    assert!(opts.retry.enabled);
    assert_eq!(opts.retry.max_attempts, 4);
    assert_eq!(opts.retry.max_duration, Some(Duration::from_secs(30)));
    assert_eq!(opts.retry.jitter_factor, 0.25);
    assert_eq!(opts.retry.method_policy, HttpRetryMethodPolicy::AllMethods);
    assert_eq!(
        opts.retry.delay_strategy,
        RetryDelay::Exponential {
            initial: Duration::from_millis(50),
            max: Duration::from_secs(2),
            multiplier: 1.5,
        }
    );
}

#[test]
fn test_http_retry_options_delay_strategies_from_config() {
    let mut fixed_config = Config::new();
    fixed_config
        .set("retry.delay_strategy", "FIXED".to_string())
        .unwrap();
    fixed_config
        .set("retry.fixed_delay", Duration::from_millis(250))
        .unwrap();
    let fixed = HttpRetryOptions::from_config(&fixed_config.prefix_view("retry")).unwrap();
    assert_eq!(
        fixed.delay_strategy,
        RetryDelay::Fixed(Duration::from_millis(250))
    );

    let mut random_config = Config::new();
    random_config
        .set("retry.delay_strategy", "RANDOM".to_string())
        .unwrap();
    random_config
        .set("retry.random_min_delay", Duration::from_millis(10))
        .unwrap();
    random_config
        .set("retry.random_max_delay", Duration::from_millis(20))
        .unwrap();
    let random = HttpRetryOptions::from_config(&random_config.prefix_view("retry")).unwrap();
    assert_eq!(
        random.delay_strategy,
        RetryDelay::Random {
            min: Duration::from_millis(10),
            max: Duration::from_millis(20),
        }
    );

    let mut none_config = Config::new();
    none_config
        .set("retry.delay_strategy", "NONE".to_string())
        .unwrap();
    let none = HttpRetryOptions::from_config(&none_config.prefix_view("retry")).unwrap();
    assert_eq!(none.delay_strategy, RetryDelay::None);
}

#[test]
fn test_http_retry_options_method_policy_aliases_from_config() {
    let mut idempotent_config = Config::new();
    idempotent_config
        .set("retry.method_policy", "idempotent".to_string())
        .unwrap();
    let idempotent = HttpRetryOptions::from_config(&idempotent_config.prefix_view("retry"))
        .expect("idempotent alias should parse");
    assert_eq!(
        idempotent.method_policy,
        HttpRetryMethodPolicy::IdempotentOnly
    );

    let mut none_config = Config::new();
    none_config
        .set("retry.method_policy", "disabled".to_string())
        .unwrap();
    let none = HttpRetryOptions::from_config(&none_config.prefix_view("retry"))
        .expect("disabled alias should parse");
    assert_eq!(none.method_policy, HttpRetryMethodPolicy::None);
}

#[test]
fn test_http_retry_options_invalid_method_policy_from_config() {
    let mut config = Config::new();
    config
        .set("retry.method_policy", "unsafe-only".to_string())
        .unwrap();

    let err = HttpRetryOptions::from_config(&config.prefix_view("retry")).unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "method_policy");
}

#[test]
fn test_http_client_options_retry_section_invalid_value_is_prefixed() {
    let mut config = Config::new();
    config
        .set("http.retry.delay_strategy", "bad-strategy".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "http.retry.delay_strategy");
}

#[test]
fn test_http_client_options_sse_section() {
    let mut config = Config::new();
    config
        .set("http.sse.json_mode", "strict".to_string())
        .unwrap();
    config.set("http.sse.max_line_bytes", 8192usize).unwrap();
    config.set("http.sse.max_frame_bytes", 65536usize).unwrap();
    config
        .set("http.sse.done_marker", "disabled".to_string())
        .unwrap();

    let opts = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap();

    assert_eq!(opts.sse_json_mode, SseJsonMode::Strict);
    assert_eq!(opts.sse_done_marker_policy, DoneMarkerPolicy::Disabled);
    assert_eq!(opts.sse_max_line_bytes, 8192);
    assert_eq!(opts.sse_max_frame_bytes, 65536);
}

#[test]
fn test_http_client_options_sse_json_mode_invalid_value_is_prefixed() {
    let mut config = Config::new();
    config
        .set("http.sse.json_mode", "fail-fast".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "http.sse.json_mode");
}

#[test]
fn test_http_client_options_sse_limits_zero_is_prefixed() {
    let mut config = Config::new();
    config.set("http.sse.max_line_bytes", 0usize).unwrap();

    let err = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "http.sse.max_line_bytes");
}

#[test]
fn test_http_retry_options_validate_rejects_invalid_values() {
    let mut options = HttpRetryOptions::default();
    options.max_attempts = 0;
    let err = options.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "max_attempts");

    let mut options = HttpRetryOptions::default();
    options.jitter_factor = 1.5;
    let err = options.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "jitter_factor");

    let mut options = HttpRetryOptions::default();
    options.delay_strategy = RetryDelay::Fixed(Duration::ZERO);
    let err = options.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "delay_strategy");
}

#[test]
fn test_http_retry_method_policy_allows_methods() {
    assert!(HttpRetryMethodPolicy::IdempotentOnly.allows_method(&http::Method::GET));
    assert!(HttpRetryMethodPolicy::IdempotentOnly.allows_method(&http::Method::DELETE));
    assert!(!HttpRetryMethodPolicy::IdempotentOnly.allows_method(&http::Method::POST));
    assert!(HttpRetryMethodPolicy::AllMethods.allows_method(&http::Method::POST));
    assert!(!HttpRetryMethodPolicy::None.allows_method(&http::Method::GET));
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
fn test_http_client_options_validate_propagates_retry_error() {
    let mut opts = HttpClientOptions::default();
    opts.retry.max_attempts = 0;

    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "retry.max_attempts");
}

#[test]
fn test_http_client_options_validate_rejects_zero_error_response_preview_limit() {
    let mut opts = HttpClientOptions::default();
    opts.error_response_preview_limit = 0;

    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "error_response_preview_limit");
}

#[test]
fn test_http_client_options_validate_rejects_blank_user_agent() {
    let mut opts = HttpClientOptions::default();
    opts.user_agent = Some("   ".to_string());

    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "user_agent");
}

#[test]
fn test_http_client_options_validate_rejects_invalid_user_agent() {
    let mut opts = HttpClientOptions::default();
    opts.user_agent = Some("line1\nline2".to_string());

    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "user_agent");
}

#[test]
fn test_http_client_options_validate_propagates_timeout_error() {
    let mut opts = HttpClientOptions::default();
    opts.timeouts.connect_timeout = Duration::ZERO;

    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "timeouts.connect_timeout");
}

#[test]
fn test_http_client_options_timeout_section_zero_value_is_prefixed() {
    let mut config = Config::new();
    config
        .set("http.timeouts.connect_timeout", Duration::ZERO)
        .expect("test config should set connect_timeout");

    let err = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "http.timeouts.connect_timeout");
}

#[test]
fn test_http_client_options_validate_rejects_zero_sse_frame_limit() {
    let mut opts = HttpClientOptions::default();
    opts.sse_max_frame_bytes = 0;

    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "sse.max_frame_bytes");
}

#[test]
fn test_http_client_options_error_response_preview_limit_zero_is_prefixed() {
    let mut config = Config::new();
    config
        .set("http.error_response_preview_limit", 0usize)
        .expect("test config should set error_response_preview_limit");

    let err = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "http.error_response_preview_limit");
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

#[test]
fn test_http_client_options_sensitive_headers_invalid_type_is_prefixed() {
    let mut config = Config::new();
    config.set("http.sensitive_headers", 123_i32).unwrap();

    let err = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(err.path, "http.sensitive_headers");
}

#[test]
fn test_http_client_options_default_headers_prefers_subkey_form_over_invalid_json_form() {
    let mut config = Config::new();
    config
        .set("http.default_headers", "not-json".to_string())
        .unwrap();
    config
        .set("http.default_headers.x-api-key", "from-subkey".to_string())
        .unwrap();

    let opts = HttpClientOptions::from_config(&config.prefix_view("http")).unwrap();
    assert_eq!(opts.default_headers.len(), 1);
    assert_eq!(
        opts.default_headers.get("x-api-key").unwrap(),
        "from-subkey"
    );
}
