// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_http::sse::SseChunk;

#[test]
fn test_sse_chunk_serializes_data_and_done_variants() {
    let data = SseChunk::Data(serde_json::json!({"delta": "hello"}));
    let done: SseChunk<serde_json::Value> = SseChunk::Done;

    assert_eq!(data, SseChunk::Data(serde_json::json!({"delta": "hello"})));
    assert_eq!(
        serde_json::to_value(done).expect("done should serialize"),
        serde_json::json!("done")
    );
}
