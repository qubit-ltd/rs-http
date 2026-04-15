/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::Duration;

use http::StatusCode;
use qubit_config::Config;
use qubit_http::{HttpConfigErrorKind, HttpErrorKind, HttpRetryMethodPolicy, HttpRetryOptions};

#[test]
fn test_http_retry_options_alias_exponential_dash_normalizes_to_exponential_backoff() {
    let mut config = Config::new();
    config.set("retry.delay_strategy", "exponential").unwrap();
    config
        .set("retry.backoff_initial_delay", Duration::from_millis(40))
        .unwrap();
    config
        .set("retry.backoff_max_delay", Duration::from_secs(1))
        .unwrap();

    let options = HttpRetryOptions::from_config(&config.prefix_view("retry")).unwrap();
    assert_eq!(
        options.delay_strategy,
        qubit_retry::RetryDelay::Exponential {
            initial: Duration::from_millis(40),
            max: Duration::from_secs(1),
            multiplier: 2.0,
        }
    );
}

#[test]
fn test_http_retry_options_none_policy_disables_retries() {
    let mut options = HttpRetryOptions::default();
    options.enabled = true;
    options.method_policy = HttpRetryMethodPolicy::None;

    assert!(!options.allows_method(&http::Method::GET));
    assert!(!options.allows_method(&http::Method::POST));
}

#[test]
fn test_http_retry_options_parses_status_and_error_kind_allowlists() {
    let mut config = Config::new();
    config
        .set(
            "retry.status_codes",
            vec!["429".to_string(), "503".to_string(), "429".to_string()],
        )
        .unwrap();
    config
        .set(
            "retry.error_kinds",
            vec![
                "transport".to_string(),
                "read-timeout".to_string(),
                "transport".to_string(),
            ],
        )
        .unwrap();

    let options = HttpRetryOptions::from_config(&config.prefix_view("retry")).unwrap();
    assert_eq!(
        options.retry_status_codes,
        Some(vec![
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::SERVICE_UNAVAILABLE
        ])
    );
    assert_eq!(
        options.retry_error_kinds,
        Some(vec![HttpErrorKind::Transport, HttpErrorKind::ReadTimeout])
    );
}

#[test]
fn test_http_retry_options_rejects_invalid_status_code_in_allowlist() {
    let mut config = Config::new();
    config
        .set(
            "retry.status_codes",
            vec!["200".to_string(), "999".to_string()],
        )
        .unwrap();

    let error = HttpRetryOptions::from_config(&config.prefix_view("retry")).unwrap_err();
    assert_eq!(error.kind, HttpConfigErrorKind::InvalidValue);
    assert!(error.path.contains("status_codes"));
}

#[test]
fn test_http_retry_options_rejects_invalid_error_kind_in_allowlist() {
    let mut config = Config::new();
    config
        .set(
            "retry.error_kinds",
            vec!["transport".to_string(), "unknown-kind".to_string()],
        )
        .unwrap();

    let error = HttpRetryOptions::from_config(&config.prefix_view("retry")).unwrap_err();
    assert_eq!(error.kind, HttpConfigErrorKind::InvalidValue);
    assert!(error.path.contains("error_kinds"));
}

#[test]
fn test_http_retry_options_custom_allowlists_override_default_retryability() {
    let mut options = HttpRetryOptions::default();
    assert!(options.is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR));
    assert!(options.is_retryable_error_kind(HttpErrorKind::Transport));

    options.retry_status_codes = Some(vec![StatusCode::TOO_MANY_REQUESTS]);
    options.retry_error_kinds = Some(vec![HttpErrorKind::ReadTimeout]);

    assert!(!options.is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR));
    assert!(options.is_retryable_status(StatusCode::TOO_MANY_REQUESTS));
    assert!(!options.is_retryable_error_kind(HttpErrorKind::Transport));
    assert!(options.is_retryable_error_kind(HttpErrorKind::ReadTimeout));
}

#[test]
fn test_http_retry_options_parses_all_supported_error_kinds() {
    let mut config = Config::new();
    config
        .set(
            "retry.error_kinds",
            vec![
                "invalid_url".to_string(),
                "build_client".to_string(),
                "proxy_config".to_string(),
                "connect_timeout".to_string(),
                "read_timeout".to_string(),
                "write_timeout".to_string(),
                "request_timeout".to_string(),
                "transport".to_string(),
                "status".to_string(),
                "decode".to_string(),
                "sse_protocol".to_string(),
                "sse_decode".to_string(),
                "cancelled".to_string(),
                "retry_attempt_timeout".to_string(),
                "retry_max_elapsed_exceeded".to_string(),
                "retry_aborted".to_string(),
                "other".to_string(),
            ],
        )
        .unwrap();

    let options = HttpRetryOptions::from_config(&config.prefix_view("retry")).unwrap();
    let kinds = options
        .retry_error_kinds
        .expect("retry_error_kinds should be parsed");
    assert_eq!(kinds.len(), 17);
    assert!(kinds.contains(&HttpErrorKind::InvalidUrl));
    assert!(kinds.contains(&HttpErrorKind::BuildClient));
    assert!(kinds.contains(&HttpErrorKind::ProxyConfig));
    assert!(kinds.contains(&HttpErrorKind::ConnectTimeout));
    assert!(kinds.contains(&HttpErrorKind::ReadTimeout));
    assert!(kinds.contains(&HttpErrorKind::WriteTimeout));
    assert!(kinds.contains(&HttpErrorKind::RequestTimeout));
    assert!(kinds.contains(&HttpErrorKind::Transport));
    assert!(kinds.contains(&HttpErrorKind::Status));
    assert!(kinds.contains(&HttpErrorKind::Decode));
    assert!(kinds.contains(&HttpErrorKind::SseProtocol));
    assert!(kinds.contains(&HttpErrorKind::SseDecode));
    assert!(kinds.contains(&HttpErrorKind::Cancelled));
    assert!(kinds.contains(&HttpErrorKind::RetryAttemptTimeout));
    assert!(kinds.contains(&HttpErrorKind::RetryMaxElapsedExceeded));
    assert!(kinds.contains(&HttpErrorKind::RetryAborted));
    assert!(kinds.contains(&HttpErrorKind::Other));
}

#[test]
fn test_http_retry_options_allowlists_trim_values_and_sort_status_codes() {
    let mut config = Config::new();
    config
        .set(
            "retry.status_codes",
            vec![" 503 ".to_string(), "429".to_string(), "500".to_string()],
        )
        .unwrap();
    config
        .set(
            "retry.error_kinds",
            vec![" read-timeout ".to_string(), "transport".to_string()],
        )
        .unwrap();

    let options = HttpRetryOptions::from_config(&config.prefix_view("retry")).unwrap();
    assert_eq!(
        options.retry_status_codes,
        Some(vec![
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::SERVICE_UNAVAILABLE
        ])
    );
    assert_eq!(
        options.retry_error_kinds,
        Some(vec![HttpErrorKind::ReadTimeout, HttpErrorKind::Transport])
    );
}

#[test]
fn test_http_retry_options_rejects_blank_values_in_allowlists() {
    let mut config = Config::new();
    config
        .set(
            "retry.status_codes",
            vec!["429".to_string(), " ".to_string()],
        )
        .unwrap();

    let status_error = HttpRetryOptions::from_config(&config.prefix_view("retry")).unwrap_err();
    assert_eq!(status_error.kind, HttpConfigErrorKind::InvalidValue);
    assert!(status_error.path.contains("status_codes"));

    let mut config2 = Config::new();
    config2
        .set(
            "retry.error_kinds",
            vec!["transport".to_string(), " ".to_string()],
        )
        .unwrap();

    let kind_error = HttpRetryOptions::from_config(&config2.prefix_view("retry")).unwrap_err();
    assert_eq!(kind_error.kind, HttpConfigErrorKind::InvalidValue);
    assert!(kind_error.path.contains("error_kinds"));
}

#[test]
fn test_http_retry_options_rejects_status_code_below_100() {
    let mut config = Config::new();
    config
        .set("retry.status_codes", vec!["99".to_string()])
        .unwrap();

    let error = HttpRetryOptions::from_config(&config.prefix_view("retry")).unwrap_err();
    assert_eq!(error.kind, HttpConfigErrorKind::InvalidValue);
    assert!(error.path.contains("status_codes"));
}
