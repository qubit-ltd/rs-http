/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # Done Marker Policy
//!
//! Defines how stream completion markers are recognized.
//!
//! # Author
//!
//! Haixing Hu

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Policy for stream completion marker matching.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    Display,
    EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(ascii_case_insensitive, serialize_all = "snake_case")]
pub enum DoneMarkerPolicy {
    /// Disable done marker recognition.
    #[strum(serialize = "disable")]
    Disabled,
    /// Use default marker: `[DONE]`.
    #[default]
    #[strum(serialize = "default")]
    DefaultDone,
    /// Use a custom marker string.
    #[strum(disabled)]
    Custom(String),
}

impl DoneMarkerPolicy {
    /// Returns whether `payload` (trimmed) signals end-of-stream per this policy.
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
