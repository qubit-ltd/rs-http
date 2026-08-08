// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # Done Marker Policy
//!
//! Defines how stream completion markers are recognized.

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;

/// Policy for stream completion marker matching.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoneMarkerPolicy {
    /// Disable done marker recognition.
    Disabled,
    /// Use default marker: `[DONE]`.
    #[default]
    DefaultDone,
    /// Use a custom marker string.
    Custom(String),
}

impl fmt::Display for DoneMarkerPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => write!(f, "disable"),
            Self::DefaultDone => write!(f, "default"),
            Self::Custom(value) => write!(f, "{value}"),
        }
    }
}

impl FromStr for DoneMarkerPolicy {
    type Err = parse_display::ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let trimmed = input.trim();
        if trimmed.eq_ignore_ascii_case("disable") {
            return Ok(Self::Disabled);
        }
        if trimmed.eq_ignore_ascii_case("disabled") {
            return Ok(Self::Disabled);
        }
        if trimmed.eq_ignore_ascii_case("default") {
            return Ok(Self::DefaultDone);
        }
        Ok(Self::Custom(trimmed.to_string()))
    }
}

impl DoneMarkerPolicy {
    /// Returns whether `payload` (trimmed) signals end-of-stream per this
    /// policy.
    ///
    /// # Parameters
    /// - `payload`: Typically trimmed SSE `data:` text.
    ///
    /// # Returns
    /// `true` when the stream should stop emitting data chunks (e.g. `[DONE]`).
    pub fn is_done(&self, payload: &str) -> bool {
        match self {
            DoneMarkerPolicy::Disabled => false,
            DoneMarkerPolicy::DefaultDone => payload.trim() == "[DONE]",
            DoneMarkerPolicy::Custom(marker) => payload.trim() == marker.trim(),
        }
    }
}
