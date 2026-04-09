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
use qubit_http::TimeoutOptions;

#[test]
fn test_timeout_options_defaults_when_no_keys() {
    let config = Config::new();
    let opts = TimeoutOptions::from_config(&config.prefix_view("http.timeouts")).unwrap();
    assert_eq!(opts, TimeoutOptions::default());
}

#[test]
fn test_timeout_options_all_fields() {
    let mut config = Config::new();
    config
        .set("http.timeouts.connect_timeout", Duration::from_secs(5))
        .unwrap();
    config
        .set("http.timeouts.read_timeout", Duration::from_secs(30))
        .unwrap();
    config
        .set("http.timeouts.write_timeout", Duration::from_secs(20))
        .unwrap();
    config
        .set("http.timeouts.request_timeout", Duration::from_secs(60))
        .unwrap();

    let opts = TimeoutOptions::from_config(&config.prefix_view("http.timeouts")).unwrap();
    assert_eq!(opts.connect_timeout, Duration::from_secs(5));
    assert_eq!(opts.read_timeout, Duration::from_secs(30));
    assert_eq!(opts.write_timeout, Duration::from_secs(20));
    assert_eq!(opts.request_timeout, Some(Duration::from_secs(60)));
}

#[test]
fn test_timeout_options_partial_fields() {
    let mut config = Config::new();
    config
        .set("t.connect_timeout", Duration::from_millis(500))
        .unwrap();

    let opts = TimeoutOptions::from_config(&config.prefix_view("t")).unwrap();
    assert_eq!(opts.connect_timeout, Duration::from_millis(500));
    assert_eq!(opts.read_timeout, Duration::from_secs(120));
    assert_eq!(opts.request_timeout, None);
}

#[test]
fn test_timeout_options_no_request_timeout() {
    let mut config = Config::new();
    config
        .set("t.connect_timeout", Duration::from_secs(10))
        .unwrap();

    let opts = TimeoutOptions::from_config(&config.prefix_view("t")).unwrap();
    assert_eq!(opts.request_timeout, None);
}
