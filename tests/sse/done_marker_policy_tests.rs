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

use std::str::FromStr;

#[test]
fn test_done_marker_policy_basic_behavior() {
    assert!(DoneMarkerPolicy::DefaultDone.is_done("[DONE]"));
    assert!(!DoneMarkerPolicy::DefaultDone.is_done("[done]"));
    assert!(DoneMarkerPolicy::Custom(" END ".to_string()).is_done("END"));
    assert!(!DoneMarkerPolicy::Disabled.is_done("[DONE]"));
}

#[test]
fn test_done_marker_policy_from_str_parses_aliases_and_custom() {
    assert_eq!(
        DoneMarkerPolicy::from_str("disable").expect("disable"),
        DoneMarkerPolicy::Disabled
    );
    assert_eq!(
        DoneMarkerPolicy::from_str("  disabled ").expect("disabled"),
        DoneMarkerPolicy::Disabled
    );
    assert_eq!(
        DoneMarkerPolicy::from_str("default").expect("default"),
        DoneMarkerPolicy::DefaultDone
    );
    assert_eq!(
        DoneMarkerPolicy::from_str("[FIN]").expect("custom"),
        DoneMarkerPolicy::Custom("[FIN]".to_string())
    );
}
