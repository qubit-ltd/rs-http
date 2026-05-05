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
use qubit_http::HttpRequestBody;

#[test]
fn test_http_request_body_variants_store_expected_payloads() {
    assert_eq!(HttpRequestBody::default(), HttpRequestBody::Empty);
    assert_eq!(HttpRequestBody::Empty.to_string(), "empty");

    let text = HttpRequestBody::Text("hello".to_string());
    let bytes = HttpRequestBody::Bytes(Bytes::from_static(b"hello"));

    assert_eq!(text, HttpRequestBody::Text("hello".to_string()));
    assert_eq!(bytes, HttpRequestBody::Bytes(Bytes::from_static(b"hello")));
}
