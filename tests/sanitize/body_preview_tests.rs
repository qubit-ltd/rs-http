// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_http::LogSanitizer;

#[test]
fn test_body_preview_new_clamps_zero_limit() {
    let sanitizer = LogSanitizer::default();

    assert_eq!(
        sanitizer.sanitize_request_body_preview(b"abc", 0, None),
        "<redacted: unsupported HTTP body>...<truncated 2 bytes>"
    );
}

#[test]
fn test_body_preview_with_content_type_enables_structured_redaction() {
    let sanitizer = LogSanitizer::default();
    let body = br#"{"token":"secret","name":"alice"}"#;

    assert_eq!(
        sanitizer.sanitize_request_body_preview(
            body,
            1024,
            Some("application/json"),
        ),
        r#"{"name":"alice","token":"****"}"#
    );
}

#[test]
fn test_body_preview_invalid_content_type_keeps_empty_truncation_suffix_when_complete(
) {
    let sanitizer = LogSanitizer::default();

    assert_eq!(
        sanitizer.sanitize_request_body_preview(
            b"secret",
            1024,
            Some("bad\nvalue")
        ),
        "<redacted: invalid content type body>"
    );
}

#[test]
fn test_body_preview_invalid_content_type_uses_response_truncation_suffix() {
    let sanitizer = LogSanitizer::default();

    assert_eq!(
        sanitizer.sanitize_response_body_preview(
            b"secret",
            2,
            Some("bad\nvalue")
        ),
        "<redacted: invalid content type body>...<truncated 4 bytes>"
    );
}

#[test]
fn test_body_preview_invalid_content_type_uses_error_response_truncation_suffix(
) {
    let sanitizer = LogSanitizer::default();

    assert_eq!(
        sanitizer.sanitize_error_response_body_preview(
            b"secret",
            2,
            Some("bad\nvalue")
        ),
        "<redacted: invalid content type body>...<truncated>"
    );
}
