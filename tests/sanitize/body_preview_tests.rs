/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use qubit_http::{
    BodyLogContext,
    BodyPreview,
    LogSanitizer,
};

#[test]
fn test_body_preview_new_clamps_zero_limit() {
    let sanitizer = LogSanitizer::default();
    let preview = BodyPreview::new(b"abc", 0, BodyLogContext::Request);

    assert_eq!(
        sanitizer.sanitize_body_preview(&preview),
        "<redacted: unsupported HTTP body>...<truncated 2 bytes>"
    );
}

#[test]
fn test_body_preview_with_content_type_enables_structured_redaction() {
    let sanitizer = LogSanitizer::default();
    let preview = BodyPreview::new(
        br#"{"token":"secret","name":"alice"}"#,
        1024,
        BodyLogContext::Request,
    )
    .with_content_type("application/json");

    assert_eq!(
        sanitizer.sanitize_body_preview(&preview),
        r#"{"name":"alice","token":"****"}"#
    );
}

#[test]
fn test_body_preview_invalid_content_type_keeps_empty_truncation_suffix_when_complete() {
    let sanitizer = LogSanitizer::default();
    let preview =
        BodyPreview::new(b"secret", 1024, BodyLogContext::Request).with_content_type("bad\nvalue");

    assert_eq!(
        sanitizer.sanitize_body_preview(&preview),
        "<redacted: invalid content type body>"
    );
}

#[test]
fn test_body_preview_invalid_content_type_uses_response_truncation_suffix() {
    let sanitizer = LogSanitizer::default();
    let preview =
        BodyPreview::new(b"secret", 2, BodyLogContext::Response).with_content_type("bad\nvalue");

    assert_eq!(
        sanitizer.sanitize_body_preview(&preview),
        "<redacted: invalid content type body>...<truncated 4 bytes>"
    );
}

#[test]
fn test_body_preview_invalid_content_type_uses_error_response_truncation_suffix() {
    let sanitizer = LogSanitizer::default();
    let preview = BodyPreview::new(b"secret", 2, BodyLogContext::ErrorResponse)
        .with_content_type("bad\nvalue");

    assert_eq!(
        sanitizer.sanitize_body_preview(&preview),
        "<redacted: invalid content type body>...<truncated>"
    );
}
