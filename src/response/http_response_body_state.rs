// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use bytes::Bytes;

/// Ownership state of a response body.
#[derive(Debug)]
pub(super) enum HttpResponseBodyState {
    Backend(reqwest::Response),
    Buffered(Bytes),
    StreamingTaken,
}
