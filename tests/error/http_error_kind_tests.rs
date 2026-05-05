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

use qubit_http::HttpErrorKind;

#[test]
fn test_http_error_kind_display_from_str_and_serde_use_snake_case() {
    assert_eq!(HttpErrorKind::RequestTimeout.to_string(), "request_timeout");
    assert_eq!(
        HttpErrorKind::from_str("sse_protocol").expect("kind should parse"),
        HttpErrorKind::SseProtocol
    );

    let json =
        serde_json::to_string(&HttpErrorKind::ReadTimeout).expect("kind should serialize to JSON");
    assert_eq!(json, "\"read_timeout\"");
    let decoded: HttpErrorKind =
        serde_json::from_str("\"retry_aborted\"").expect("kind should deserialize");
    assert_eq!(decoded, HttpErrorKind::RetryAborted);
}
