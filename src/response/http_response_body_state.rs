// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use bytes::Bytes;

/// Ownership state of a response body.
#[derive(Debug)]
pub(super) enum HttpResponseBodyState {
    /// Backend response whose body has not been consumed.
    Backend(reqwest::Response),
    /// Fully buffered response body.
    Buffered(Bytes),
    /// Streaming body ownership has been transferred to the caller.
    StreamingTaken,
}
