/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Tests for `src/sse/done_marker_policy.rs`.

use qubit_http::sse::DoneMarkerPolicy;

#[test]
fn test_done_marker_policy_basic_behavior() {
    assert!(DoneMarkerPolicy::DefaultDone.is_done("[DONE]"));
    assert!(!DoneMarkerPolicy::DefaultDone.is_done("[done]"));
    assert!(DoneMarkerPolicy::Custom(" END ".to_string()).is_done("END"));
    assert!(!DoneMarkerPolicy::Disabled.is_done("[DONE]"));
}
