/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use qubit_http::SensitiveHttpHeaders;

#[test]
fn test_sensitive_http_headers_deduplicates_trimmed_case_insensitive_names() {
    let mut headers = SensitiveHttpHeaders::new();
    headers.insert(" Authorization ");
    headers.insert("authorization");
    headers.insert("AUTHORIZATION");
    headers.insert("X-Secret");

    assert_eq!(headers.len(), 2);
    assert!(headers.contains("authorization"));
    assert!(headers.contains("x-secret"));
    assert_eq!(
        headers.iter().collect::<Vec<_>>(),
        vec!["authorization", "x-secret"]
    );
}
