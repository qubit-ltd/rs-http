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
use qubit_http::HttpResponse;
use url::Url;

#[tokio::test]
async fn test_message_decoder_decodes_multiple_sse_messages_from_response_body() {
    let response = HttpResponse::new(
        StatusCode::OK,
        HeaderMap::new(),
        Bytes::from_static(b"id: 1\nevent: add\ndata: one\n\nid: 2\ndata: two\n\n"),
        Url::parse("https://example.com/events").expect("valid URL"),
        Method::GET,
    );

    let messages = response
        .sse_messages()
        .map(|item| item.expect("message should decode"))
        .collect::<Vec<_>>()
        .await;

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].last_event_id.as_deref(), Some("1"));
    assert_eq!(messages[0].event.as_deref(), Some("add"));
    assert_eq!(messages[0].data, "one");
    assert_eq!(messages[1].last_event_id.as_deref(), Some("2"));
    assert_eq!(messages[1].data, "two");
}
