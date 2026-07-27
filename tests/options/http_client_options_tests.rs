// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use qubit_config::{
    options::ReadOptions,
    Config,
};
use qubit_datatype::DataType;
use qubit_http::{
    constants::{
        DEFAULT_CONNECT_TIMEOUT_SECS,
        DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES,
        DEFAULT_LOG_BODY_SIZE_LIMIT_BYTES,
        DEFAULT_READ_TIMEOUT_SECS,
        DEFAULT_RESPONSE_BODY_SIZE_LIMIT_BYTES,
        DEFAULT_SSE_MAX_FRAME_BYTES,
        DEFAULT_SSE_MAX_LINE_BYTES,
        DEFAULT_WRITE_TIMEOUT_SECS,
    },
    sse::{
        DoneMarkerPolicy,
        SseJsonMode,
    },
    HttpClientOptions,
    HttpConfigErrorKind,
    HttpErrorKind,
    HttpRetryMethodPolicy,
    HttpRetryOptions,
    LogRedactionPolicy,
    ProxyType,
};
use qubit_redact::{
    http::UrlPathPolicy,
    Sensitivity,
};
use qubit_retry::RetryDelay;

/// Verifies HTTP option parsing only reads process environment placeholders
/// through an explicitly environment-friendly reader.
#[test]
fn test_http_client_options_requires_explicit_environment_fallback() {
    const ENV_NAME: &str = "QUBIT_HTTP_CONFIG_EXPLICIT_ENV_FALLBACK";

    unsafe {
        std::env::set_var(ENV_NAME, "Bearer test-token");
    }

    let mut config = Config::new();
    config
        .set(
            "http.default_headers.authorization",
            format!("${{{ENV_NAME}}}"),
        )
        .expect("test configuration should accept the placeholder");
    let section = config
        .section("http")
        .expect("the HTTP section path should be canonical");

    let default_result = HttpClientOptions::from_config(&section);
    let read_options = ReadOptions::env_friendly();
    let env_view = section.with_read_options_view(&read_options);
    let explicit_result = HttpClientOptions::from_config(&env_view);

    unsafe {
        std::env::remove_var(ENV_NAME);
    }

    assert!(
        default_result.is_err(),
        "default HTTP configuration must not read process environment"
    );
    let options = explicit_result
        .expect("explicit environment fallback should resolve the placeholder");
    assert_eq!(
        options.default_headers["authorization"],
        "Bearer test-token"
    );
}

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
    assert_eq!(
        options.response_body_size_limit,
        DEFAULT_RESPONSE_BODY_SIZE_LIMIT_BYTES
    );
    assert_eq!(options.user_agent, None);
    assert_eq!(options.max_redirects, None);
    assert_eq!(options.pool_idle_timeout, None);
    assert_eq!(options.pool_max_idle_per_host, None);
    assert!(!options.use_env_proxy);
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
    assert_eq!(
        options.response_body_size_limit,
        defaults.response_body_size_limit
    );
    assert_eq!(options.user_agent, defaults.user_agent);
    assert_eq!(options.max_redirects, defaults.max_redirects);
    assert_eq!(options.pool_idle_timeout, defaults.pool_idle_timeout);
    assert_eq!(
        options.pool_max_idle_per_host,
        defaults.pool_max_idle_per_host
    );
    assert_eq!(options.use_env_proxy, defaults.use_env_proxy);
    assert_eq!(options.retry, defaults.retry);
    assert_eq!(options.log_redaction_policy, defaults.log_redaction_policy);
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
fn test_http_client_options_debug_masks_sensitive_values() {
    let mut options = HttpClientOptions::new();
    options
        .set_base_url(
            "https://debug-user:debug-url-secret@example.com/root/?accessToken=debug-query-secret",
        )
        .expect("base URL should be valid");
    options
        .add_header("authorization", "Bearer debug-secret-token")
        .expect("sensitive default header should be accepted");
    options.proxy.enabled = true;
    options.proxy.host = Some("proxy.example.com".to_string());
    options.proxy.port = Some(8080);
    options.proxy.username = Some("debug-user".to_string());
    options.proxy.password = Some("debug-proxy-secret".to_string());

    let debug = format!("{options:?}");

    assert!(!debug.contains("debug-secret-token"));
    assert!(!debug.contains("debug-url-secret"));
    assert!(!debug.contains("debug-query-secret"));
    assert!(!debug.contains("debug-proxy-secret"));
    assert!(debug.contains("authorization"));
    assert!(debug.contains("****"));
}

#[test]
fn test_http_client_options_debug_honors_explicit_sensitivity_override() {
    let mut options = HttpClientOptions::new();
    options.log_redaction_policy = LogRedactionPolicy::builder()
        .override_header("authorization", Sensitivity::Low)
        .build()
        .expect("log redaction policy should be valid");
    options
        .add_header("authorization", "Bearer downgrade-secret")
        .expect("sensitive default header should be accepted");

    let debug = format!("{options:?}");

    assert!(
        debug.contains("authorization: [Be****et]"),
        "unexpected debug output: {debug}",
    );
    assert!(!debug.contains("Bearer downgrade-secret"));
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
fn test_http_client_options_invalid_ipv4_only_type_is_prefixed() {
    let mut config = Config::new();
    config.set("http.ipv4_only", "maybe".to_string()).unwrap();

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(err.path, "http.ipv4_only");
}

#[test]
fn test_http_client_options_reqwest_extra_fields_from_config() {
    let mut config = Config::new();
    config
        .set("http.user_agent", "qubit-http-tests/1.0".to_string())
        .unwrap();
    config.set("http.max_redirects", 7u64).unwrap();
    config
        .set("http.pool_idle_timeout", Duration::from_secs(15))
        .unwrap();
    config.set("http.pool_max_idle_per_host", 32u64).unwrap();
    config.set("http.use_env_proxy", true).unwrap();

    let opts = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap();
    assert_eq!(opts.user_agent.as_deref(), Some("qubit-http-tests/1.0"));
    assert_eq!(opts.max_redirects, Some(7));
    assert_eq!(opts.pool_idle_timeout, Some(Duration::from_secs(15)));
    assert_eq!(opts.pool_max_idle_per_host, Some(32));
    assert!(opts.use_env_proxy);
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

    let opts = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap();
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

    let opts = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap();
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

    let opts = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap();
    assert_eq!(opts.default_headers.len(), 2);
}

#[test]
fn test_http_client_options_default_headers_invalid_json() {
    let mut config = Config::new();
    config
        .set("http.default_headers", "not-json".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(
        err.message,
        "Failed to parse default_headers JSON: Failed to parse JSON at normalized line 1 column 2",
    );
}

#[test]
fn test_http_client_options_default_headers_json_form_rejects_markdown_fence() {
    let mut config = Config::new();
    config
        .set(
            "http.default_headers",
            "```json\n{\"x-app-id\":\"my-app\"}\n```".to_string(),
        )
        .unwrap();

    let error =
        HttpClientOptions::from_config(&config.section("http").unwrap())
            .expect_err(
                "strict default_headers JSON must reject Markdown fences",
            );
    assert_eq!(error.kind, HttpConfigErrorKind::TypeError);
    assert!(error
        .message
        .starts_with("Failed to parse default_headers JSON:"));
}

#[test]
fn test_http_client_options_invalid_header_name() {
    let mut config = Config::new();
    config
        .set("http.default_headers.invalid header", "value".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidHeader);
    assert_eq!(err.path, "http.default_headers.invalid header");
}

#[test]
fn test_http_client_options_invalid_header_value_from_config() {
    let mut config = Config::new();
    config
        .set("http.default_headers.x-bad", "line1\nline2".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::InvalidHeader);
    assert_eq!(err.path, "http.default_headers.x-bad");
}

#[test]
fn test_http_client_options_numeric_header_value_from_config_is_converted() {
    let mut config = Config::new();
    config.set("http.default_headers.x-number", 42_i32).unwrap();

    let opts = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap();

    assert_eq!(opts.default_headers.get("x-number").unwrap(), "42");
}

#[test]
fn test_http_client_options_empty_header_value_from_config_is_prefixed() {
    let mut config = Config::new();
    config
        .set_null("http.default_headers.x-empty", DataType::String)
        .unwrap();

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::ConfigError);
    assert_eq!(err.path, "http.default_headers.x-empty");
}

#[test]
fn test_http_client_options_add_header_and_add_headers_are_atomic() {
    let mut opts = HttpClientOptions::new();
    opts.add_header("x-app", "qubit").unwrap();
    opts.add_headers(&[("x-env", "test"), ("x-region", "cn")])
        .unwrap();

    assert_eq!(opts.default_headers.get("x-app").unwrap(), "qubit");
    assert_eq!(opts.default_headers.get("x-env").unwrap(), "test");
    assert_eq!(opts.default_headers.get("x-region").unwrap(), "cn");

    let error = opts
        .add_headers(&[("x-keep", "value"), ("bad header", "boom")])
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
fn test_http_client_options_log_redaction_section() {
    let mut config = Config::new();
    config
        .set(
            "http.log_redaction.sensitive_headers",
            vec!["X-Custom-Secret".to_string(), "X-Api-Token".to_string()],
        )
        .unwrap();
    config
        .set(
            "http.log_redaction.sensitive_query_params",
            vec!["session_token".to_string()],
        )
        .unwrap();
    config
        .set(
            "http.log_redaction.sensitive_body_fields",
            vec!["customer_secret".to_string()],
        )
        .unwrap();
    config
        .set(
            "http.log_redaction.excluded_sensitive_headers",
            vec!["Authorization".to_string()],
        )
        .unwrap();
    config
        .set(
            "http.log_redaction.excluded_sensitive_query_params",
            vec!["sig".to_string()],
        )
        .unwrap();
    config
        .set(
            "http.log_redaction.excluded_sensitive_body_fields",
            vec!["signature".to_string()],
        )
        .unwrap();
    config
        .set("http.log_redaction.url_path_policy", "preserve".to_string())
        .unwrap();

    let opts = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap();
    assert!(opts
        .log_redaction_policy
        .http_policy()
        .header_policy()
        .sensitivity_for("x-custom-secret")
        .is_some());
    assert!(opts
        .log_redaction_policy
        .http_policy()
        .header_policy()
        .sensitivity_for("x-api-token")
        .is_some());
    assert!(opts
        .log_redaction_policy
        .http_policy()
        .header_policy()
        .sensitivity_for("authorization")
        .is_none());
    assert!(opts
        .log_redaction_policy
        .http_policy()
        .query_policy()
        .sensitivity_for("session_token")
        .is_some());
    assert!(opts
        .log_redaction_policy
        .http_policy()
        .query_policy()
        .sensitivity_for("access_token")
        .is_some());
    assert!(opts
        .log_redaction_policy
        .http_policy()
        .body_policy()
        .sensitivity_for("customer_secret")
        .is_some());
    assert!(opts
        .log_redaction_policy
        .http_policy()
        .body_policy()
        .sensitivity_for("client_secret")
        .is_some());
    assert!(opts
        .log_redaction_policy
        .http_policy()
        .query_policy()
        .sensitivity_for("sig")
        .is_none());
    assert!(opts
        .log_redaction_policy
        .http_policy()
        .body_policy()
        .sensitivity_for("signature")
        .is_none());
    assert_eq!(
        opts.log_redaction_policy.http_policy().url_path_policy(),
        UrlPathPolicy::Preserve,
    );
}

/// Verifies the explicit `redact` URL-path setting survives policy building.
#[test]
fn test_http_client_options_parses_redact_url_path_policy() {
    let mut config = Config::new();
    config
        .set("http.log_redaction.url_path_policy", "redact".to_string())
        .expect("test configuration should accept URL path policy text");

    let options =
        HttpClientOptions::from_config(&config.section("http").unwrap())
            .expect("redact URL path policy should parse");

    assert_eq!(
        options.log_redaction_policy.http_policy().url_path_policy(),
        UrlPathPolicy::Redact,
    );
}

#[test]
fn test_http_client_options_rejects_invalid_url_path_policy() {
    let mut config = Config::new();
    config
        .set("http.log_redaction.url_path_policy", "visible".to_string())
        .unwrap();

    let error =
        HttpClientOptions::from_config(&config.section("http").unwrap())
            .unwrap_err();

    assert_eq!(error.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(error.path, "http.log_redaction.url_path_policy");
}

#[test]
fn test_http_client_options_rejects_invalid_redaction_list_fields_with_resolved_path(
) {
    for field in [
        "sensitive_headers",
        "sensitive_query_params",
        "sensitive_body_fields",
        "excluded_sensitive_headers",
        "excluded_sensitive_query_params",
        "excluded_sensitive_body_fields",
    ] {
        let path = format!("http.log_redaction.{field}");
        let mut config = Config::new();
        config
            .set(&path, vec!["---".to_owned()])
            .expect("test configuration should accept a string list");

        let error =
            HttpClientOptions::from_config(&config.section("http").unwrap())
                .expect_err("separator-only names must be rejected");

        assert_eq!(error.path, path);
        assert_eq!(error.kind, HttpConfigErrorKind::InvalidValue);
    }
}

#[test]
fn test_http_client_options_root_sensitive_headers_is_not_supported() {
    let mut config = Config::new();
    config
        .set(
            "http.sensitive_headers",
            vec!["X-Legacy-Secret".to_string()],
        )
        .unwrap();

    let opts = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap();

    assert!(opts
        .log_redaction_policy
        .http_policy()
        .header_policy()
        .sensitivity_for("xlegacysecret")
        .is_none());
}

#[test]
fn test_http_client_options_proxy_section() {
    let mut config = Config::new();
    config.set("http.proxy.enabled", true).unwrap();
    config
        .set("http.proxy.host", "proxy.corp.example.com".to_string())
        .unwrap();
    config.set("http.proxy.port", 3128u16).unwrap();

    let opts = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap();
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

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "http.proxy.proxy_type");
}

#[test]
fn test_http_client_options_logging_section() {
    let mut config = Config::new();
    config.set("http.logging.enabled", false).unwrap();
    config.set("http.logging.body_size_limit", 8192u64).unwrap();

    let opts = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap();
    assert!(!opts.logging.enabled);
    assert_eq!(opts.logging.body_size_limit, 8192);
}

#[test]
fn test_http_client_options_error_response_preview_limit_from_config() {
    let mut config = Config::new();
    config
        .set("http.error_response_preview_limit", 512u64)
        .expect("test config should set error_response_preview_limit");

    let opts = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap();
    assert_eq!(opts.error_response_preview_limit, 512);
}

#[test]
fn test_http_client_options_response_body_size_limit_from_config() {
    let mut config = Config::new();
    config
        .set("http.response_body_size_limit", 512u64)
        .expect("test config should set response_body_size_limit");

    let opts = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap();
    assert_eq!(opts.response_body_size_limit, 512);
}

#[test]
fn test_http_client_options_logging_section_type_error_is_prefixed() {
    let mut config = Config::new();
    config
        .set("http.logging.enabled", "not-bool".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();

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

    let opts = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap();

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
    let fixed =
        HttpRetryOptions::from_config(&fixed_config.section("retry").unwrap())
            .unwrap();
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
    let random =
        HttpRetryOptions::from_config(&random_config.section("retry").unwrap())
            .unwrap();
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
    let none =
        HttpRetryOptions::from_config(&none_config.section("retry").unwrap())
            .unwrap();
    assert_eq!(none.delay_strategy, RetryDelay::None);
}

#[test]
fn test_http_retry_options_method_policy_aliases_from_config() {
    let mut idempotent_config = Config::new();
    idempotent_config
        .set("retry.method_policy", "idempotent".to_string())
        .unwrap();
    let idempotent = HttpRetryOptions::from_config(
        &idempotent_config.section("retry").unwrap(),
    )
    .expect("idempotent alias should parse");
    assert_eq!(
        idempotent.method_policy,
        HttpRetryMethodPolicy::IdempotentOnly
    );

    let mut none_config = Config::new();
    none_config
        .set("retry.method_policy", "disabled".to_string())
        .unwrap();
    let none =
        HttpRetryOptions::from_config(&none_config.section("retry").unwrap())
            .expect("disabled alias should parse");
    assert_eq!(none.method_policy, HttpRetryMethodPolicy::None);
}

#[test]
fn test_http_retry_options_invalid_method_policy_from_config() {
    let mut config = Config::new();
    config
        .set("retry.method_policy", "unsafe-only".to_string())
        .unwrap();

    let err = HttpRetryOptions::from_config(&config.section("retry").unwrap())
        .unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "method_policy");
}

#[test]
fn test_http_client_options_retry_section_invalid_value_is_prefixed() {
    let mut config = Config::new();
    config
        .set("http.retry.delay_strategy", "bad-strategy".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "http.retry.delay_strategy");
}

#[test]
fn test_http_client_options_sse_section() {
    let mut config = Config::new();
    config
        .set("http.sse.json_mode", "strict".to_string())
        .unwrap();
    config.set("http.sse.max_line_bytes", 8192u64).unwrap();
    config.set("http.sse.max_frame_bytes", 65536u64).unwrap();
    config
        .set("http.sse.done_marker", "disabled".to_string())
        .unwrap();

    let opts = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap();

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

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "http.sse.json_mode");
}

#[test]
fn test_http_client_options_sse_done_marker_default_alias_and_custom_value() {
    let mut default_config = Config::new();
    default_config
        .set("http.sse.done_marker", " default ".to_string())
        .unwrap();

    let default_opts = HttpClientOptions::from_config(
        &default_config.section("http").unwrap(),
    )
    .unwrap();
    assert_eq!(
        default_opts.sse_done_marker_policy,
        DoneMarkerPolicy::DefaultDone
    );

    let mut custom_config = Config::new();
    custom_config
        .set("http.sse.done_marker", "  [FIN]  ".to_string())
        .unwrap();

    let custom_opts =
        HttpClientOptions::from_config(&custom_config.section("http").unwrap())
            .unwrap();
    assert_eq!(
        custom_opts.sse_done_marker_policy,
        DoneMarkerPolicy::Custom("[FIN]".to_string())
    );
}

#[test]
fn test_http_client_options_sse_done_marker_empty_value_is_prefixed() {
    let mut config = Config::new();
    config
        .set("http.sse.done_marker", "   ".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "http.sse.done_marker");
}

#[test]
fn test_http_client_options_sse_limits_zero_is_prefixed() {
    let mut config = Config::new();
    config.set("http.sse.max_line_bytes", 0u64).unwrap();

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "http.sse.max_line_bytes");
}

#[test]
fn test_http_client_options_sse_max_frame_zero_is_prefixed() {
    let mut config = Config::new();
    config.set("http.sse.max_frame_bytes", 0u64).unwrap();

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "http.sse.max_frame_bytes");
}

#[test]
fn test_http_client_options_sse_max_frame_invalid_type_is_prefixed() {
    let mut config = Config::new();
    config
        .set("http.sse.max_frame_bytes", "large".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(err.path, "http.sse.max_frame_bytes");
}

#[test]
fn test_http_client_options_default_headers_map_invalid_type_is_prefixed() {
    let mut config = Config::new();
    config.set("http.default_headers", 42_i32).unwrap();

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::TypeError);
    assert_eq!(err.path, "http.default_headers");
}

#[test]
fn test_http_client_options_default_headers_map_substitution_error_is_prefixed()
{
    let mut config = Config::new();
    config
        .set(
            "http.default_headers",
            "${QUBIT_HTTP_UNSET_DEFAULT_HEADERS_JSON}".to_string(),
        )
        .unwrap();

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();

    assert_eq!(err.kind, HttpConfigErrorKind::ConfigError);
    assert_eq!(err.path, "http.default_headers");
}

#[test]
fn test_http_retry_options_validate_rejects_invalid_values() {
    let mut options = HttpRetryOptions::default();
    options.max_attempts = 0;
    let err = options.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "max_attempts");
    assert_eq!(err.message, "Retry max_attempts must be greater than 0",);

    let mut options = HttpRetryOptions::default();
    options.jitter_factor = 1.5;
    let err = options.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "jitter_factor");
    assert_eq!(
        err.message,
        "Retry jitter_factor must be between 0.0 and 1.0",
    );

    let mut options = HttpRetryOptions::default();
    options.delay_strategy = RetryDelay::Fixed(Duration::ZERO);
    let err = options.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "delay_strategy");
}

#[test]
fn test_http_retry_options_validate_rejects_non_finite_jitter() {
    for jitter_factor in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let options = HttpRetryOptions {
            jitter_factor,
            ..Default::default()
        };

        let error = options.validate().unwrap_err();

        assert_eq!(error.kind, HttpConfigErrorKind::InvalidValue);
        assert_eq!(error.path, "jitter_factor");
        assert_eq!(
            error.message,
            "Retry jitter_factor must be between 0.0 and 1.0",
        );
    }
}

#[test]
fn test_http_retry_options_validate_reports_first_invalid_field() {
    let options = HttpRetryOptions {
        max_attempts: 0,
        jitter_factor: f64::NAN,
        delay_strategy: RetryDelay::Fixed(Duration::ZERO),
        ..Default::default()
    };

    let error = options.validate().unwrap_err();

    assert_eq!(error.path, "max_attempts");
    assert_eq!(error.message, "Retry max_attempts must be greater than 0",);
}

#[test]
fn test_http_retry_method_policy_allows_methods() {
    assert!(
        HttpRetryMethodPolicy::IdempotentOnly.allows_method(&http::Method::GET)
    );
    assert!(HttpRetryMethodPolicy::IdempotentOnly
        .allows_method(&http::Method::DELETE));
    assert!(!HttpRetryMethodPolicy::IdempotentOnly
        .allows_method(&http::Method::POST));
    assert!(
        HttpRetryMethodPolicy::AllMethods.allows_method(&http::Method::POST)
    );
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
    assert_eq!(err.message, "Retry max_attempts must be greater than 0",);
    assert_eq!(
        err.to_string(),
        "[invalid value] retry.max_attempts: Retry max_attempts must be greater than 0",
    );
}

#[test]
fn test_http_client_options_validate_rejects_zero_error_response_preview_limit()
{
    let mut opts = HttpClientOptions::default();
    opts.error_response_preview_limit = 0;

    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "error_response_preview_limit");
    assert_eq!(err.message, "Value must be greater than 0");
    assert_eq!(
        err.to_string(),
        "[invalid value] error_response_preview_limit: Value must be greater than 0",
    );
}

#[test]
fn test_http_client_options_validate_rejects_zero_response_body_size_limit() {
    let mut opts = HttpClientOptions::default();
    opts.response_body_size_limit = 0;

    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "response_body_size_limit");
    assert_eq!(err.message, "Value must be greater than 0");
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
    assert_eq!(err.message, "Timeout value must be greater than 0");
    assert_eq!(
        err.to_string(),
        "[invalid value] timeouts.connect_timeout: Timeout value must be greater than 0",
    );
}

#[test]
fn test_http_client_options_timeout_section_zero_value_is_prefixed() {
    let mut config = Config::new();
    config
        .set("http.timeouts.connect_timeout", Duration::ZERO)
        .expect("test config should set connect_timeout");

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();
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
fn test_http_client_options_validate_rejects_zero_sse_line_limit() {
    let mut opts = HttpClientOptions::default();
    opts.sse_max_line_bytes = 0;

    let err = opts.validate().unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "sse.max_line_bytes");
}

#[test]
fn test_http_client_options_error_response_preview_limit_zero_is_prefixed() {
    let mut config = Config::new();
    config
        .set("http.error_response_preview_limit", 0u64)
        .expect("test config should set error_response_preview_limit");

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();
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
fn test_http_client_options_from_root_config_all_sections() {
    let mut config = Config::new();
    config
        .set("base_url", "https://root.example.com/api/".to_string())
        .unwrap();
    config.set("ipv4_only", true).unwrap();
    config.set("error_response_preview_limit", 1024u64).unwrap();
    config
        .set("user_agent", "qubit-http-root/1.0".to_string())
        .unwrap();
    config.set("max_redirects", 5u64).unwrap();
    config
        .set("pool_idle_timeout", Duration::from_secs(20))
        .unwrap();
    config.set("pool_max_idle_per_host", 9u64).unwrap();
    config.set("use_env_proxy", true).unwrap();
    config
        .set("default_headers.x-root", "root-value".to_string())
        .unwrap();
    config
        .set(
            "log_redaction.sensitive_headers",
            vec!["X-Root-Secret".to_string()],
        )
        .unwrap();
    config
        .set(
            "log_redaction.sensitive_query_params",
            vec!["root_token".to_string()],
        )
        .unwrap();
    config
        .set(
            "log_redaction.sensitive_body_fields",
            vec!["root_password".to_string()],
        )
        .unwrap();
    config
        .set("timeouts.connect_timeout", Duration::from_secs(3))
        .unwrap();
    config
        .set("timeouts.read_timeout", Duration::from_secs(4))
        .unwrap();
    config
        .set("timeouts.write_timeout", Duration::from_secs(5))
        .unwrap();
    config
        .set("timeouts.request_timeout", Duration::from_secs(6))
        .unwrap();
    config.set("proxy.enabled", true).unwrap();
    config
        .set("proxy.host", "proxy.root.example".to_string())
        .unwrap();
    config.set("proxy.port", 8080u16).unwrap();
    config
        .set("proxy.username", "root-user".to_string())
        .unwrap();
    config
        .set("proxy.password", "root-pass".to_string())
        .unwrap();
    config.set("logging.enabled", false).unwrap();
    config.set("logging.log_request_header", false).unwrap();
    config.set("logging.log_request_body", false).unwrap();
    config.set("logging.log_response_header", false).unwrap();
    config.set("logging.log_response_body", false).unwrap();
    config.set("logging.body_size_limit", 2048u64).unwrap();
    config.set("retry.enabled", true).unwrap();
    config.set("retry.max_attempts", 2u32).unwrap();
    config
        .set("retry.delay_strategy", "fixed".to_string())
        .unwrap();
    config
        .set("retry.fixed_delay", Duration::from_millis(25))
        .unwrap();
    config
        .set("retry.status_codes", vec!["503".to_string()])
        .unwrap();
    config
        .set("retry.error_kinds", vec!["transport".to_string()])
        .unwrap();
    config.set("sse.json_mode", "strict".to_string()).unwrap();
    config.set("sse.done_marker", "[END]".to_string()).unwrap();
    config.set("sse.max_line_bytes", 4096u64).unwrap();
    config.set("sse.max_frame_bytes", 8192u64).unwrap();

    let opts = HttpClientOptions::from_config(&config).unwrap();

    assert_eq!(
        opts.base_url.expect("base_url should be set").as_str(),
        "https://root.example.com/api/"
    );
    assert!(opts.ipv4_only);
    assert_eq!(opts.error_response_preview_limit, 1024);
    assert_eq!(opts.user_agent.as_deref(), Some("qubit-http-root/1.0"));
    assert_eq!(opts.max_redirects, Some(5));
    assert_eq!(opts.pool_idle_timeout, Some(Duration::from_secs(20)));
    assert_eq!(opts.pool_max_idle_per_host, Some(9));
    assert!(opts.use_env_proxy);
    assert!(opts.default_headers.contains_key("x-root"));
    assert!(opts
        .log_redaction_policy
        .http_policy()
        .header_policy()
        .sensitivity_for("x-root-secret")
        .is_some());
    assert!(opts
        .log_redaction_policy
        .http_policy()
        .query_policy()
        .sensitivity_for("root_token")
        .is_some());
    assert!(opts
        .log_redaction_policy
        .http_policy()
        .body_policy()
        .sensitivity_for("root_password")
        .is_some());
    assert_eq!(opts.timeouts.connect_timeout, Duration::from_secs(3));
    assert_eq!(opts.timeouts.request_timeout, Some(Duration::from_secs(6)));
    assert!(opts.proxy.enabled);
    assert_eq!(opts.proxy.username.as_deref(), Some("root-user"));
    assert!(!opts.logging.enabled);
    assert!(!opts.logging.log_request_header);
    assert!(opts.retry.enabled);
    assert_eq!(
        opts.retry.retry_status_codes,
        Some(vec![http::StatusCode::SERVICE_UNAVAILABLE])
    );
    assert_eq!(
        opts.retry.retry_error_kinds,
        Some(vec![HttpErrorKind::Transport])
    );
    assert_eq!(opts.sse_json_mode, SseJsonMode::Strict);
    assert_eq!(
        opts.sse_done_marker_policy,
        DoneMarkerPolicy::Custom("[END]".to_string())
    );
    assert_eq!(opts.sse_max_line_bytes, 4096);
    assert_eq!(opts.sse_max_frame_bytes, 8192);
}

#[test]
fn test_http_client_options_interpolates_string_configuration_values() {
    let mut config = Config::new();
    config
        .set("http.shared.base_url", "https://interpolated.example/api/")
        .expect("test config should set shared base URL");
    config
        .set("http.shared.user_agent", "qubit-http-interpolated/1.0")
        .expect("test config should set shared user agent");
    config
        .set("http.proxy.shared.proxy_type", "http")
        .expect("test config should set shared proxy type");
    config
        .set("http.proxy.shared.proxy_host", "proxy.interpolated.example")
        .expect("test config should set shared proxy host");
    config
        .set("http.proxy.shared.proxy_username", "interpolated-user")
        .expect("test config should set shared proxy username");
    config
        .set("http.proxy.shared.proxy_password", "interpolated-password")
        .expect("test config should set shared proxy password");
    config
        .set("http.retry.shared.retry_delay_strategy", "fixed")
        .expect("test config should set shared retry delay strategy");
    config
        .set("http.retry.shared.retry_method_policy", "all_methods")
        .expect("test config should set shared retry method policy");
    config
        .set("http.sse.shared.sse_json_mode", "strict")
        .expect("test config should set shared SSE JSON mode");
    config
        .set("http.sse.shared.sse_done_marker", "[INTERPOLATED_DONE]")
        .expect("test config should set shared SSE done marker");
    config
        .set("http.log_redaction.shared.url_path_policy", "preserve")
        .expect("test config should set shared URL path policy");
    config
        .set(
            "http.log_redaction.shared.sensitive_header",
            "X-Interpolated-Secret",
        )
        .expect("test config should set shared sensitive header");
    config
        .set("http.retry.shared.retry_status_code", "503")
        .expect("test config should set shared retry status code");
    config
        .set("http.retry.shared.retry_error_kind", "transport")
        .expect("test config should set shared retry error kind");
    config
        .set("http.base_url", "${shared.base_url}")
        .expect("test config should set interpolated base URL");
    config
        .set("http.user_agent", "${shared.user_agent}")
        .expect("test config should set interpolated user agent");
    config
        .set("http.proxy.enabled", true)
        .expect("test config should enable proxy");
    config
        .set("http.proxy.proxy_type", "${shared.proxy_type}")
        .expect("test config should set interpolated proxy type");
    config
        .set("http.proxy.host", "${shared.proxy_host}")
        .expect("test config should set interpolated proxy host");
    config
        .set("http.proxy.port", 8080u16)
        .expect("test config should set proxy port");
    config
        .set("http.proxy.username", "${shared.proxy_username}")
        .expect("test config should set interpolated proxy username");
    config
        .set("http.proxy.password", "${shared.proxy_password}")
        .expect("test config should set interpolated proxy password");
    config
        .set("http.retry.enabled", true)
        .expect("test config should enable retry");
    config
        .set(
            "http.retry.delay_strategy",
            "${shared.retry_delay_strategy}",
        )
        .expect("test config should set interpolated retry delay strategy");
    config
        .set("http.retry.fixed_delay", Duration::from_millis(25))
        .expect("test config should set retry fixed delay");
    config
        .set("http.retry.method_policy", "${shared.retry_method_policy}")
        .expect("test config should set interpolated retry method policy");
    config
        .set("http.sse.json_mode", "${shared.sse_json_mode}")
        .expect("test config should set interpolated SSE JSON mode");
    config
        .set("http.sse.done_marker", "${shared.sse_done_marker}")
        .expect("test config should set interpolated SSE done marker");
    config
        .set(
            "http.log_redaction.url_path_policy",
            "${shared.url_path_policy}",
        )
        .expect("test config should set interpolated URL path policy");
    config
        .set(
            "http.log_redaction.sensitive_headers",
            vec!["${shared.sensitive_header}"],
        )
        .expect("test config should set interpolated sensitive headers");
    config
        .set(
            "http.retry.status_codes",
            vec!["${shared.retry_status_code}"],
        )
        .expect("test config should set interpolated retry status codes");
    config
        .set("http.retry.error_kinds", vec!["${shared.retry_error_kind}"])
        .expect("test config should set interpolated retry error kinds");

    let options =
        HttpClientOptions::from_config(&config.section("http").unwrap())
            .expect("interpolated HTTP options should be valid");

    assert_eq!(
        options
            .base_url
            .expect("base URL should be configured")
            .as_str(),
        "https://interpolated.example/api/",
    );
    assert_eq!(
        options.user_agent.as_deref(),
        Some("qubit-http-interpolated/1.0"),
    );
    assert_eq!(options.proxy.proxy_type, ProxyType::Http);
    assert_eq!(
        options.proxy.host.as_deref(),
        Some("proxy.interpolated.example"),
    );
    assert_eq!(options.proxy.username.as_deref(), Some("interpolated-user"));
    assert_eq!(
        options.proxy.password.as_deref(),
        Some("interpolated-password"),
    );
    assert_eq!(
        options.retry.delay_strategy,
        RetryDelay::Fixed(Duration::from_millis(25)),
    );
    assert_eq!(
        options.retry.method_policy,
        HttpRetryMethodPolicy::AllMethods,
    );
    assert_eq!(options.sse_json_mode, SseJsonMode::Strict);
    assert_eq!(
        options.sse_done_marker_policy,
        DoneMarkerPolicy::Custom("[INTERPOLATED_DONE]".to_string()),
    );
    assert_eq!(
        options.log_redaction_policy.http_policy().url_path_policy(),
        UrlPathPolicy::Preserve,
    );
    assert!(options
        .log_redaction_policy
        .http_policy()
        .header_policy()
        .sensitivity_for("x-interpolated-secret")
        .is_some());
    assert_eq!(
        options.retry.retry_status_codes,
        Some(vec![http::StatusCode::SERVICE_UNAVAILABLE]),
    );
    assert_eq!(
        options.retry.retry_error_kinds,
        Some(vec![HttpErrorKind::Transport]),
    );
}

#[test]
fn test_http_client_options_log_redaction_header_number_from_config_is_converted(
) {
    let mut config = Config::new();
    config
        .set("http.log_redaction.sensitive_headers", 123_i32)
        .unwrap();

    let opts = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap();

    assert!(opts
        .log_redaction_policy
        .http_policy()
        .header_policy()
        .sensitivity_for("123")
        .is_some());
}

#[test]
fn test_http_client_options_default_headers_conflicting_forms_are_rejected() {
    let mut config = Config::new();
    config
        .set("http.default_headers", "not-json".to_string())
        .unwrap();
    config
        .set("http.default_headers.x-api-key", "from-subkey".to_string())
        .unwrap();

    let err = HttpClientOptions::from_config(&config.section("http").unwrap())
        .unwrap_err();
    assert_eq!(err.kind, HttpConfigErrorKind::InvalidValue);
    assert_eq!(err.path, "http.default_headers");
    assert!(err.message.contains("cannot be used at the same time"));
}
