// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_http::sse::SseChunk;
use serde_json::Value;
use serde_json::json;
use serde_json::to_value;

#[test]
fn test_sse_chunk_serializes_data_and_done_variants() {
    let data = SseChunk::Data(json!({"delta": "hello"}));
    let done: SseChunk<Value> = SseChunk::Done;

    assert_eq!(data, SseChunk::Data(json!({"delta": "hello"})));
    assert_eq!(to_value(done).expect("done should serialize"), json!("done"));
}
