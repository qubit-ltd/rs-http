// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use bytes::Bytes;
use reqwest::Response;

/// Tracks ownership and availability of an HTTP response body.
#[derive(Debug)]
pub(in crate::response) enum HttpResponseBodyState {
    /// Backend response whose body has not been consumed.
    Backend(Response),
    /// Fully buffered response body.
    Buffered(Bytes),
    /// Streaming body ownership has been transferred to the caller.
    StreamingTaken,
}
