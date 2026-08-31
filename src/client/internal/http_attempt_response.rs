// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Successful HTTP attempt state handed back to the retry facade.

use crate::HttpResponse;

/// Successful attempt plus token ownership needed by retry handoff.
pub(in crate::client) struct HttpAttemptResponse {
    /// Successful response produced by this attempt.
    response: HttpResponse,

    /// Whether the response must inherit the retry-flow token.
    restore_retry_flow_token: bool,
}

impl HttpAttemptResponse {
    /// Creates a successful attempt handoff.
    ///
    /// # Parameters
    /// - `response`: Successful response produced by the attempt.
    /// - `restore_retry_flow_token`: Whether the retry facade must attach its
    ///   original flow token to the response.
    ///
    /// # Returns
    /// A handoff containing the successful response and token restoration
    /// decision.
    pub(in crate::client) fn new(response: HttpResponse, restore_retry_flow_token: bool) -> Self {
        Self {
            response,
            restore_retry_flow_token,
        }
    }

    /// Consumes the handoff into the response and ownership decision.
    ///
    /// # Returns
    /// `(response, restore_retry_flow_token)`.
    pub(in crate::client) fn into_parts(self) -> (HttpResponse, bool) {
        (self.response, self.restore_retry_flow_token)
    }
}
