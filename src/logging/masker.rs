/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # Header Value Masking
//!
//! Provides masking utility for sensitive header values.
//!
//! # Author
//!
//! Haixing Hu

use crate::constants::{
    SENSITIVE_HEADER_MASK_EDGE_CHARS, SENSITIVE_HEADER_MASK_PLACEHOLDER,
    SENSITIVE_HEADER_MASK_SHORT_LEN,
};
use crate::SensitiveHeaders;

/// Returns a log-safe copy of `value` when `name` is listed in `sensitive_headers`.
///
/// # Parameters
/// - `name`: Header name (case-insensitive match against the set).
/// - `value`: Raw header value.
/// - `sensitive_headers`: Names that trigger masking.
///
/// # Returns
/// Original `value` if not sensitive; otherwise `****` when length ≤ 4, else first two + `****` + last two graphemes as chars.
///
/// Rules for sensitive values:
/// - length ≤ 4: `****`
/// - otherwise: first 2 characters + `****` + last 2 characters
pub fn mask_header_value(name: &str, value: &str, sensitive_headers: &SensitiveHeaders) -> String {
    if value.is_empty() {
        return String::new();
    }
    if !sensitive_headers.contains(name) {
        return value.to_string();
    }

    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= SENSITIVE_HEADER_MASK_SHORT_LEN {
        SENSITIVE_HEADER_MASK_PLACEHOLDER.to_string()
    } else {
        let edge = SENSITIVE_HEADER_MASK_EDGE_CHARS;
        let prefix: String = chars[..edge].iter().collect();
        let suffix: String = chars[chars.len() - edge..].iter().collect();
        format!("{prefix}{SENSITIVE_HEADER_MASK_PLACEHOLDER}{suffix}")
    }
}
