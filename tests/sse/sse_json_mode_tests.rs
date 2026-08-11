// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::str::FromStr;

use qubit_http::sse::SseJsonMode;

#[test]
fn test_sse_json_mode_parses_case_insensitive_and_serializes_snake_case() {
    assert_eq!(
        SseJsonMode::from_str("LENIENT").expect("mode should parse"),
        SseJsonMode::Lenient
    );
    assert_eq!(
        SseJsonMode::from_str("strict").expect("mode should parse"),
        SseJsonMode::Strict
    );
    assert_eq!(
        serde_json::to_string(&SseJsonMode::Strict)
            .expect("mode should serialize"),
        "\"strict\""
    );
}
