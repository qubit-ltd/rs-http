// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::str::FromStr;

use qubit_http::HttpError;
use qubit_http::HttpErrorKind;
use qubit_http::RetryHint;

#[test]
fn test_http_error_kind_display_from_str_and_serde_use_snake_case() {
    assert_eq!(HttpErrorKind::RequestTimeout.to_string(), "request_timeout");
    assert_eq!(
        HttpErrorKind::from_str("sse_protocol").expect("kind should parse"),
        HttpErrorKind::SseProtocol
    );
    assert_eq!(
        HttpErrorKind::from_str("redirect_limit_exceeded").expect("kind should parse"),
        HttpErrorKind::RedirectLimitExceeded
    );
    assert_eq!(
        HttpErrorKind::RedirectLimitExceeded.to_string(),
        "redirect_limit_exceeded"
    );

    let json = serde_json::to_string(&HttpErrorKind::ReadTimeout).expect("kind should serialize to JSON");
    assert_eq!(json, "\"read_timeout\"");
    let decoded: HttpErrorKind = serde_json::from_str("\"retry_budget_exceeded\"").expect("kind should deserialize");
    assert_eq!(decoded, HttpErrorKind::RetryBudgetExceeded);
    let decoded: HttpErrorKind =
        serde_json::from_str("\"redirect_limit_exceeded\"").expect("redirect limit kind should deserialize");
    assert_eq!(decoded, HttpErrorKind::RedirectLimitExceeded);
}

#[test]
fn test_redirect_limit_error_is_non_retryable_by_hint() {
    let error = HttpError::new(HttpErrorKind::RedirectLimitExceeded, "redirect limit exceeded");
    assert_eq!(error.retry_hint(), RetryHint::NonRetryable);
}
