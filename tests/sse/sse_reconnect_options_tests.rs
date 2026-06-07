// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_http::sse::SseReconnectOptions;

#[test]
fn test_sse_reconnect_options_default_enables_expected_reconnect_behavior() {
    let options = SseReconnectOptions::new();

    assert_eq!(options.retry.max_attempts(), 4);
    assert!(options.reconnect_on_eof);
    assert!(options.honor_server_retry);
    assert_eq!(options.server_retry_max_delay, None);
    assert!(options.apply_jitter_to_server_retry);
}
