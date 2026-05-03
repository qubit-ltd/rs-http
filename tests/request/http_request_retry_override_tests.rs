/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use qubit_http::{HttpRequestRetryOverride, HttpRetryMethodPolicy};

#[test]
fn test_request_retry_override_default_follows_client_policy() {
    let override_options = HttpRequestRetryOverride::new();

    assert!(!override_options.is_force_enable());
    assert!(!override_options.is_force_disable());
    assert_eq!(override_options.method_policy(), None);
    assert!(!override_options.should_honor_retry_after());
}

#[test]
fn test_request_retry_override_force_flags_are_mutually_exclusive() {
    let force_enable = HttpRequestRetryOverride::new().force_enable();
    assert!(force_enable.is_force_enable());
    assert!(!force_enable.is_force_disable());

    let force_disable = force_enable.force_disable();
    assert!(!force_disable.is_force_enable());
    assert!(force_disable.is_force_disable());
}

#[test]
fn test_request_retry_override_method_policy_and_retry_after_flags() {
    let override_options = HttpRequestRetryOverride::new()
        .with_method_policy(HttpRetryMethodPolicy::AllMethods)
        .with_honor_retry_after(true);

    assert_eq!(
        override_options.method_policy(),
        Some(HttpRetryMethodPolicy::AllMethods)
    );
    assert!(override_options.should_honor_retry_after());
}
