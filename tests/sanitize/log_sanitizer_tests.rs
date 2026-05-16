/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use bytes::Bytes;
use http::header::{
    HeaderName,
    HeaderValue,
    AUTHORIZATION,
};
use qubit_http::{
    BodyLogContext,
    BodyPreview,
    LogSanitizePolicy,
    LogSanitizer,
};
use url::Url;

#[test]
fn test_log_sanitizer_sanitize_url_masks_sensitive_query_params() {
    let sanitizer = LogSanitizer::default();
    let url = Url::parse("https://example.com/search?q=rust&access_token=secret-token")
        .expect("test URL should parse");

    let sanitized = sanitizer.sanitize_url(&url);

    assert_eq!(
        sanitized,
        "https://example.com/search?q=rust&access_token=****"
    );
}

#[test]
fn test_log_sanitizer_sanitize_url_masks_camel_case_sensitive_query_params() {
    let sanitizer = LogSanitizer::default();
    let url = Url::parse(
        "https://api.example.com/search?accessToken=secret-access&clientSecret=secret-client",
    )
    .expect("test URL should parse");

    let sanitized = sanitizer.sanitize_url(&url);

    assert_eq!(
        sanitized,
        "https://api.example.com/search?accessToken=****&clientSecret=%3Credacted%3E"
    );
    assert!(!sanitized.contains("secret-access"));
    assert!(!sanitized.contains("secret-client"));
}

#[test]
fn test_log_sanitizer_sanitize_url_uses_shared_secret_level_for_password_query() {
    let sanitizer = LogSanitizer::default();
    let url =
        Url::parse("https://example.com/login?password=secret&access_token=raw-access-secret")
            .expect("test URL should parse");

    let sanitized = sanitizer.sanitize_url(&url);

    assert_eq!(
        sanitized,
        "https://example.com/login?password=%3Credacted%3E&access_token=****"
    );
    assert!(!sanitized.contains("secret"));
}

#[test]
fn test_log_sanitizer_policy_returns_underlying_policy() {
    let sanitizer = LogSanitizer::default();

    assert!(sanitizer
        .policy()
        .sensitive_headers
        .contains("authorization"));
}

#[test]
fn test_log_sanitizer_sanitize_url_masks_password() {
    let sanitizer = LogSanitizer::default();
    let url = Url::parse("https://alice:secret-password@example.com/search?q=rust")
        .expect("test URL should parse");

    let sanitized = sanitizer.sanitize_url(&url);

    assert_eq!(sanitized, "https://****:****@example.com/search?q=rust");
    assert!(!sanitized.contains("alice"));
    assert!(!sanitized.contains("secret-password"));
}

#[test]
fn test_log_sanitizer_sanitize_url_masks_userinfo_and_fragment() {
    let sanitizer = LogSanitizer::default();
    let url = Url::parse(
        "https://api-token:secret-password@example.com/callback?access_token=query-secret#id_token=fragment-secret",
    )
    .expect("test URL should parse");

    let sanitized = sanitizer.sanitize_url(&url);

    assert_eq!(
        sanitized,
        "https://****:****@example.com/callback?access_token=****#****"
    );
    assert!(!sanitized.contains("api-token"));
    assert!(!sanitized.contains("secret-password"));
    assert!(!sanitized.contains("query-secret"));
    assert!(!sanitized.contains("fragment-secret"));
}

#[test]
fn test_log_sanitizer_sanitize_header_masks_configured_header_names() {
    let sanitizer = LogSanitizer::default();

    let sanitized = sanitizer.sanitize_header_value(
        &AUTHORIZATION,
        &HeaderValue::from_static("Bearer very-secret-token"),
    );

    assert_eq!(sanitized, "****");
}

#[test]
fn test_log_sanitizer_sanitize_header_keeps_non_sensitive_header_values() {
    let sanitizer = LogSanitizer::default();
    let header_name = HeaderName::from_static("content-type");

    let sanitized = sanitizer
        .sanitize_header_value(&header_name, &HeaderValue::from_static("application/json"));

    assert_eq!(sanitized, "application/json");
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_json_fields() {
    let sanitizer = LogSanitizer::default();
    let body =
        Bytes::from_static(br#"{"user":"alice","password":"secret","nested":{"token":"abc"}}"#);
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("application/json");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(
        sanitized,
        r#"{"nested":{"token":"****"},"password":"<redacted>","user":"alice"}"#
    );
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_camel_case_json_fields() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        br#"{"accessToken":"secret-access","clientSecret":"secret-client","user":"alice"}"#,
    );
    let preview = BodyPreview::new(body.as_ref(), 1024, BodyLogContext::Request);

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(
        sanitized,
        r#"{"accessToken":"****","clientSecret":"<redacted>","user":"alice"}"#
    );
    assert!(!sanitized.contains("secret-access"));
    assert!(!sanitized.contains("secret-client"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_does_not_leak_truncated_json() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(br#"{"password":"secret","user":"alice","tail":"long"}"#);
    let preview =
        BodyPreview::new(&body, 20, BodyLogContext::Request).with_content_type("application/json");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.starts_with("<redacted: invalid or truncated JSON>"));
    assert!(!sanitized.contains("secret"));
}

#[test]
fn test_log_sanitizer_error_response_truncated_json_uses_status_error_suffix() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(br#"{"password":"secret","user":"alice","tail":"long"}"#);
    let preview = BodyPreview::new(&body, 20, BodyLogContext::ErrorResponse)
        .with_content_type("application/json");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(
        sanitized,
        "<redacted: invalid or truncated JSON>...<truncated>"
    );
    assert!(!sanitized.contains("secret"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_json_arrays() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(br#"[{"token":"abc"},{"nested":{"password":"secret"}}]"#);
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("application/json");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(
        sanitized,
        r#"[{"token":"****"},{"nested":{"password":"<redacted>"}}]"#
    );
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_ndjson_fields() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        br#"{"token":"abc","id":1}

{"id":2}"#,
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("application/x-ndjson");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(sanitized, "{\"id\":1,\"token\":\"****\"}\n\n{\"id\":2}");
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_does_not_leak_truncated_ndjson() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(br#"{"token":"abc","id":1}"#);
    let preview = BodyPreview::new(&body, 10, BodyLogContext::Request)
        .with_content_type("application/x-ndjson");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.starts_with("<redacted: invalid or truncated NDJSON>"));
    assert!(!sanitized.contains("abc"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_multipart_form_fields() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        b"--boundary\r\n\
          Content-Disposition: form-data; name=\"username\"\r\n\
          \r\n\
          alice\r\n\
          --boundary\r\n\
          Content-Disposition: form-data; name=\"password\"\r\n\
          \r\n\
          secret-password\r\n\
          --boundary--\r\n",
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/form-data; boundary=boundary");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.contains("username=alice"));
    assert!(sanitized.contains("password=<redacted>"));
    assert!(!sanitized.contains("secret-password"));
    assert!(!sanitized.contains("boundary"));
    assert!(!sanitized.contains("<redacted: multipart body>"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_multipart_mixed_fields() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        b"--boundary\r\n\
          Content-Disposition: form-data; name=\"password\"\r\n\
          \r\n\
          secret-password\r\n\
          --boundary--\r\n",
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/mixed; boundary=boundary");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.contains("password=<redacted>"));
    assert!(!sanitized.contains("secret-password"));
    assert!(!sanitized.contains("boundary"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_multipart_mixed_without_boundary() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        b"--boundary\r\n\
          Content-Disposition: form-data; name=\"password\"\r\n\
          \r\n\
          secret-password\r\n\
          --boundary--\r\n",
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/mixed");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(sanitized, "<redacted: multipart body>");
    assert!(!sanitized.contains("secret-password"));
    assert!(!sanitized.contains("boundary"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_accepts_multipart_boundary_after_malformed_parameter() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        b"--boundary\r\n\
          Content-Disposition: form-data; name=\"username\"\r\n\
          \r\n\
          alice\r\n\
          --boundary--\r\n",
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/form-data; charset; boundary=boundary");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.contains("username=alice"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_multipart_json_part() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        br#"--boundary
Content-Disposition: form-data; name="metadata"
Content-Type: application/json

{"token":"secret-token","visible":"ok"}
--boundary--
"#,
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/form-data; boundary=boundary");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.contains(r#"metadata={"token":"****","visible":"ok"}"#));
    assert!(!sanitized.contains("secret-token"));
    assert!(!sanitized.contains("boundary"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_keeps_multipart_text_part() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        b"--boundary\r\n\
          Content-Disposition: form-data; name=\"description\"\r\n\
          Content-Type: text/plain\r\n\
          \r\n\
          plain text value\r\n\
          --boundary--\r\n",
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/form-data; boundary=boundary");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.contains("description=plain text value"));
    assert!(!sanitized.contains("boundary"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_keeps_multipart_text_containing_boundary_text() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        b"--boundary\r\n\
          Content-Disposition: form-data; name=\"description\"\r\n\
          Content-Type: text/plain\r\n\
          \r\n\
          plain text mentions --boundary inside the value\r\n\
          --boundary--\r\n",
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/form-data; boundary=boundary");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.contains("description=plain text mentions --boundary inside the value"));
    assert!(!sanitized.contains("<redacted: multipart body>"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_invalid_multipart_json_part() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        b"--boundary\r\n\
          Content-Disposition: form-data; name=\"metadata\"\r\n\
          Content-Type: application/json\r\n\
          \r\n\
          {\"token\":\"secret-token\"\r\n\
          --boundary--\r\n",
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/form-data; boundary=boundary");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.contains("metadata=<redacted: multipart part>"));
    assert!(!sanitized.contains("secret-token"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_invalid_multipart_ndjson_part() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        b"--boundary\r\n\
          Content-Disposition: form-data; name=\"events\"\r\n\
          Content-Type: application/x-ndjson\r\n\
          \r\n\
          {\"token\":\"secret-token\"\n\
          --boundary--\r\n",
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/form-data; boundary=boundary");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.contains("events=<redacted: multipart part>"));
    assert!(!sanitized.contains("secret-token"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_multipart_form_part() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        b"--boundary\r\n\
          Content-Disposition: form-data; name=\"payload\"\r\n\
          Content-Type: application/x-www-form-urlencoded\r\n\
          \r\n\
          username=alice&password=secret-password\r\n\
          --boundary--\r\n",
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/form-data; boundary=boundary");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.contains("payload=username=alice&password=%3Credacted%3E"));
    assert!(!sanitized.contains("secret-password"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_unknown_multipart_part_content_type() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        b"--boundary\r\n\
          Content-Disposition: form-data; name=\"payload\"\r\n\
          Content-Type: application/octet-stream\r\n\
          \r\n\
          secret-binary-looking-content\r\n\
          --boundary--\r\n",
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/form-data; boundary=boundary");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.contains("payload=<redacted: multipart part>"));
    assert!(!sanitized.contains("secret-binary-looking-content"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_multipart_file_part() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        b"--boundary\r\n\
          Content-Disposition: form-data; name=\"upload\"; filename=\"alice\\\";private-report.txt\"\r\n\
          Content-Type: text/plain\r\n\
          \r\n\
          password=secret-in-file\r\n\
          --boundary--\r\n",
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/form-data; boundary=boundary");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.contains("upload=<redacted: file part>"));
    assert!(!sanitized.contains("alice"));
    assert!(!sanitized.contains("secret-in-file"));
    assert!(!sanitized.contains("boundary"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_handles_empty_multipart_body() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(b"--boundary\r\n\r\n\r\n--boundary--\r\n");
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/form-data; boundary=boundary");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(sanitized, "<multipart>\n</multipart>");
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_non_utf8_multipart() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        b"--boundary\r\nContent-Disposition: form-data; name=\"password\"\r\n\r\nsecret-\xff\r\n--boundary--\r\n",
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/form-data; boundary=boundary");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(sanitized, "<redacted: multipart body>");
    assert!(!sanitized.contains("secret"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_malformed_multipart_body() {
    let sanitizer = LogSanitizer::default();
    let cases: [(&str, &'static [u8], &str, &str); 7] = [
        (
            "missing closing delimiter",
            b"--boundary\r\nContent-Disposition: form-data; name=\"password\"\r\n\r\nsecret",
            "multipart/form-data; boundary=boundary",
            "<redacted: multipart body>",
        ),
        (
            "malformed closing delimiter",
            b"--boundary\r\nContent-Disposition: form-data; name=\"password\"\r\n\r\nsecret\r\n--boundary--extra",
            "multipart/form-data; boundary=boundary",
            "<redacted: multipart body>",
        ),
        (
            "malformed part header",
            b"--boundary\r\nContent-Disposition form-data; name=\"password\"\r\n\r\nsecret\r\n--boundary--",
            "multipart/form-data; boundary=boundary",
            "<redacted: multipart body>",
        ),
        (
            "empty boundary",
            b"--boundary\r\nContent-Disposition: form-data; name=\"password\"\r\n\r\nsecret\r\n--boundary--",
            "multipart/form-data; boundary=\"\"",
            "<redacted: multipart body>",
        ),
        (
            "unclosed boundary quote",
            b"--boundary\r\nContent-Disposition: form-data; name=\"password\"\r\n\r\nsecret\r\n--boundary--",
            "multipart/form-data; boundary=\"boundary",
            "<redacted: multipart body>",
        ),
        (
            "trailing text after quoted boundary",
            b"--boundary\r\nContent-Disposition: form-data; name=\"password\"\r\n\r\nsecret\r\n--boundary--",
            "multipart/form-data; boundary=\"boundary\"x",
            "<redacted: multipart body>",
        ),
        (
            "control character in boundary",
            b"--boundary\r\nContent-Disposition: form-data; name=\"password\"\r\n\r\nsecret\r\n--boundary--",
            "multipart/form-data; boundary=\"bad\nboundary\"",
            "<redacted: invalid content type body>",
        ),
    ];

    for (label, body, content_type, expected) in cases {
        let bytes = Bytes::from_static(body);
        let preview = BodyPreview::new(&bytes, bytes.len(), BodyLogContext::Request)
            .with_content_type(content_type);

        let sanitized = sanitizer.sanitize_body_preview(&preview);

        assert_eq!(sanitized, expected, "{label}");
        assert!(!sanitized.contains("secret"), "{label}");
    }
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_truncated_multipart() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        b"--boundary\r\n\
          Content-Disposition: form-data; name=\"password\"\r\n\
          \r\n\
          secret-password-in-truncated-body\r\n\
          --boundary--\r\n",
    );
    let preview = BodyPreview::new(&body, 72, BodyLogContext::Request)
        .with_content_type("multipart/form-data; boundary=boundary");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert!(sanitized.starts_with("<redacted: multipart body>...<truncated "));
    assert!(!sanitized.contains("secret-password-in-truncated-body"));
    assert!(!sanitized.contains("boundary"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_multipart_without_boundary() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(
        b"--boundary\r\nContent-Disposition: form-data; name=\"password\"\r\n\r\nsecret\r\n--boundary--",
    );
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/form-data");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(sanitized, "<redacted: multipart body>");
    assert!(!sanitized.contains("secret"));
    assert!(!sanitized.contains("boundary"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_prefers_multipart_over_json_sniffing() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(br#"{"password":"secret"}"#);
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("multipart/mixed");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(sanitized, "<redacted: multipart body>");
    assert!(!sanitized.contains("secret"));
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_redacts_form_fields() {
    let sanitizer = LogSanitizer::default();
    let body = Bytes::from_static(b"username=alice&password=secret&city=Shanghai");
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("application/x-www-form-urlencoded");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(
        sanitized,
        "username=alice&password=%3Credacted%3E&city=Shanghai"
    );
}

#[test]
fn test_log_sanitizer_sanitize_body_preview_uses_custom_policy() {
    let mut policy = LogSanitizePolicy::default();
    policy.sensitive_body_fields.clear();
    policy.sensitive_body_fields.insert("customer_id");
    let sanitizer = LogSanitizer::new(policy);
    let body = Bytes::from_static(br#"{"customer_id":"C-001","password":"kept"}"#);
    let preview = BodyPreview::new(&body, body.len(), BodyLogContext::Request)
        .with_content_type("application/json");

    let sanitized = sanitizer.sanitize_body_preview(&preview);

    assert_eq!(sanitized, r#"{"customer_id":"****","password":"kept"}"#);
}
