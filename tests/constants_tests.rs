/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use qubit_http::constants::{
    DEFAULT_CONNECT_TIMEOUT_SECS,
    DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES,
    DEFAULT_LOG_BODY_SIZE_LIMIT_BYTES,
    DEFAULT_READ_TIMEOUT_SECS,
    DEFAULT_SENSITIVE_BODY_FIELD_NAMES,
    DEFAULT_SENSITIVE_HEADER_NAMES,
    DEFAULT_SENSITIVE_QUERY_PARAM_NAMES,
    DEFAULT_SSE_MAX_FRAME_BYTES,
    DEFAULT_SSE_MAX_LINE_BYTES,
    DEFAULT_WRITE_TIMEOUT_SECS,
};

#[test]
fn test_constants_keep_expected_http_defaults() {
    assert_eq!(DEFAULT_CONNECT_TIMEOUT_SECS, 10);
    assert_eq!(DEFAULT_READ_TIMEOUT_SECS, 120);
    assert_eq!(DEFAULT_WRITE_TIMEOUT_SECS, 120);
    assert_eq!(DEFAULT_LOG_BODY_SIZE_LIMIT_BYTES, 16 * 1024);
    assert_eq!(DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES, 16 * 1024);
    assert_eq!(DEFAULT_SSE_MAX_LINE_BYTES, 64 * 1024);
    assert_eq!(DEFAULT_SSE_MAX_FRAME_BYTES, 1024 * 1024);
}

#[test]
fn test_constants_expose_default_sensitive_names() {
    assert!(DEFAULT_SENSITIVE_HEADER_NAMES.contains(&"authorization"));
    assert!(DEFAULT_SENSITIVE_HEADER_NAMES.contains(&"set_cookie"));
    assert!(DEFAULT_SENSITIVE_QUERY_PARAM_NAMES.contains(&"access_token"));
    assert!(DEFAULT_SENSITIVE_QUERY_PARAM_NAMES.contains(&"client_secret"));
    assert!(DEFAULT_SENSITIVE_BODY_FIELD_NAMES.contains(&"password"));
    assert!(DEFAULT_SENSITIVE_BODY_FIELD_NAMES.contains(&"refresh_token"));
}
