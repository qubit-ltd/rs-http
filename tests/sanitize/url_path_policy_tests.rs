// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_http::UrlPathPolicy;

#[test]
fn test_url_path_policy_defaults_to_redact() {
    assert_eq!(UrlPathPolicy::default(), UrlPathPolicy::Redact);
}

#[test]
fn test_url_path_policy_is_copy_and_equatable() {
    let policy = UrlPathPolicy::Redact;
    let copied = policy;

    assert_eq!(policy, UrlPathPolicy::Redact);
    assert_eq!(copied, policy);
}
