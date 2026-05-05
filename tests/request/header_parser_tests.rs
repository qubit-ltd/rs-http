/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use http::Method;
use qubit_http::{
    HttpClientFactory,
    HttpErrorKind,
};

#[test]
fn test_header_parser_rejects_invalid_builder_header_name() {
    let client = HttpClientFactory::new()
        .create_default()
        .expect("default client should be created");

    let error = client
        .request(Method::GET, "https://example.com/")
        .header("invalid header", "value")
        .expect_err("invalid header name should fail");

    assert_eq!(error.kind, HttpErrorKind::Other);
    assert!(error.message.contains("Invalid header name"));
}
