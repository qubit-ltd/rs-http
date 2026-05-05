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
    DEFAULT_SENSITIVE_HEADER_NAMES,
    DEFAULT_SSE_MAX_FRAME_BYTES,
    DEFAULT_SSE_MAX_LINE_BYTES,
    DEFAULT_WRITE_TIMEOUT_SECS,
    SENSITIVE_HEADER_MASK_EDGE_CHARS,
    SENSITIVE_HEADER_MASK_PLACEHOLDER,
    SENSITIVE_HEADER_MASK_SHORT_LEN,
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
    assert!(DEFAULT_SENSITIVE_HEADER_NAMES.contains(&"Authorization"));
    assert!(DEFAULT_SENSITIVE_HEADER_NAMES.contains(&"Set-Cookie"));
    assert_eq!(SENSITIVE_HEADER_MASK_SHORT_LEN, 4);
    assert_eq!(SENSITIVE_HEADER_MASK_EDGE_CHARS, 2);
    assert_eq!(SENSITIVE_HEADER_MASK_PLACEHOLDER, "****");
}
