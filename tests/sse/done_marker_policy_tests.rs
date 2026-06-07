// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
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

#[test]
fn test_done_marker_policy_display_formats_variants() {
    assert_eq!(DoneMarkerPolicy::Disabled.to_string(), "disable");
    assert_eq!(DoneMarkerPolicy::DefaultDone.to_string(), "default");
    assert_eq!(
        DoneMarkerPolicy::Custom("[FIN]".to_string()).to_string(),
        "[FIN]"
    );
}
