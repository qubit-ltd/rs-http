// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Controls whether a JSON SSE stream may end at ordinary EOF.
///
/// # Examples
///
/// ```
/// let mut options = qubit_http::HttpClientOptions::new();
/// options.sse_completion_policy =
///     qubit_http::sse::SseCompletionPolicy::RequireDoneMarker;
/// assert!(matches!(
///     options.sse_completion_policy,
///     qubit_http::sse::SseCompletionPolicy::RequireDoneMarker
/// ));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum SseCompletionPolicy {
    /// Accept ordinary EOF after the last complete event.
    #[default]
    AllowEof,
    /// Require a matching done marker before EOF.
    RequireDoneMarker,
}
