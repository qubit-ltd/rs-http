// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Scalar values read from the `sse` configuration section.
pub(in crate::options) struct HttpClientSseConfigInput {
    /// Optional JSON decoding mode name.
    pub(in crate::options) json_mode: Option<String>,
    /// Optional custom done-marker value.
    pub(in crate::options) done_marker: Option<String>,
    /// Optional stream completion policy name.
    pub(in crate::options) completion: Option<String>,
    /// Optional maximum line size in bytes.
    pub(in crate::options) max_line_bytes: Option<usize>,
    /// Optional maximum frame size in bytes.
    pub(in crate::options) max_frame_bytes: Option<usize>,
}
