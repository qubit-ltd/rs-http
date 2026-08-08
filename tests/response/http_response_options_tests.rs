// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use bytes::Bytes;
use futures_util::StreamExt;
use http::HeaderMap;
use http::Method;
use http::StatusCode;
use qubit_http::HttpErrorKind;
use qubit_http::HttpResponse;
use url::Url;

#[tokio::test]
async fn test_http_response_options_clamp_sse_line_limit_to_at_least_one() {
    let response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"data: ok\n\n"),
        Url::parse("https://example.com/sse").expect("valid URL"),
        Method::GET,
    );

    let error = response
        .sse_max_line_bytes(0)
        .sse_messages()
        .next()
        .await
        .expect("stream should yield an item")
        .expect_err("line longer than clamped limit should fail");

    assert_eq!(error.kind, HttpErrorKind::SseProtocol);
    assert!(error.message.contains("max_line_bytes"));
}
