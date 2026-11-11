// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_config::Config;
use qubit_http::HttpLoggingOptions;

#[test]
fn test_http_logging_options_from_config_preserves_defaults_for_missing_flags() {
    let mut config = Config::new();
    config
        .set("logging.log_request_body", false)
        .expect("test config should set flag");

    let options = HttpLoggingOptions::from_config(&config.section("logging").unwrap()).expect("read logging");

    assert!(options.enabled);
    assert!(options.log_request_header);
    assert!(!options.log_request_body);
    assert!(options.log_response_header);
    assert!(options.log_response_body);
}
