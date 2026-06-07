// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Body logging call site used to render truncation markers consistently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyLogContext {
    /// Request body TRACE preview.
    Request,
    /// Response body TRACE preview.
    Response,
    /// Non-success status error body preview.
    ErrorResponse,
}
