/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use qubit_http::logging::mask_header_value;
use qubit_http::SensitiveHeaders;

#[test]
fn test_mask_header_value_non_sensitive_header() {
    let sensitive_headers = SensitiveHeaders::default();
    let result = mask_header_value("Content-Type", "application/json", &sensitive_headers);
    assert_eq!(result, "application/json");
}

#[test]
fn test_mask_header_value_sensitive_short_value() {
    let sensitive_headers = SensitiveHeaders::default();
    let result = mask_header_value("Authorization", "abc", &sensitive_headers);
    assert_eq!(result, "****");
}

#[test]
fn test_mask_header_value_sensitive_exactly_four_chars() {
    let sensitive_headers = SensitiveHeaders::default();
    let result = mask_header_value("Authorization", "abcd", &sensitive_headers);
    assert_eq!(result, "****");
}

#[test]
fn test_mask_header_value_sensitive_long_value() {
    let sensitive_headers = SensitiveHeaders::default();
    let result = mask_header_value("Authorization", "abcdefghijk", &sensitive_headers);
    assert_eq!(result, "ab****jk");
}

#[test]
fn test_mask_header_value_sensitive_case_insensitive() {
    let sensitive_headers = SensitiveHeaders::default();
    let result = mask_header_value("x-api-key", "1234567890", &sensitive_headers);
    assert_eq!(result, "12****90");
}

#[test]
fn test_mask_header_value_empty_value_kept_empty() {
    let sensitive_headers = SensitiveHeaders::default();
    let result = mask_header_value("Authorization", "", &sensitive_headers);
    assert_eq!(result, "");
}
