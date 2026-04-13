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
use qubit_http::{HttpRetryMethodPolicy, HttpRetryOptions};

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
        qubit_retry::Delay::Exponential {
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
