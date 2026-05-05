/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::str::FromStr;

use http::Method;
use qubit_http::HttpRetryMethodPolicy;

#[test]
fn test_http_retry_method_policy_parses_aliases_and_checks_methods() {
    assert_eq!(
        HttpRetryMethodPolicy::from_str("idempotent").expect("alias should parse"),
        HttpRetryMethodPolicy::IdempotentOnly
    );
    assert!(HttpRetryMethodPolicy::IdempotentOnly.allows_method(&Method::GET));
    assert!(!HttpRetryMethodPolicy::IdempotentOnly.allows_method(&Method::POST));
    assert!(HttpRetryMethodPolicy::AllMethods.allows_method(&Method::PATCH));
    assert!(!HttpRetryMethodPolicy::None.allows_method(&Method::DELETE));
}
