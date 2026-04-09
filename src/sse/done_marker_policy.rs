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

/// Policy for stream completion marker matching.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DoneMarkerPolicy {
    /// Disable done marker recognition.
    Disabled,
    /// Use default marker: `[DONE]`.
    #[default]
    DefaultDone,
    /// Use a custom marker string.
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
