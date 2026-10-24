// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Parsed timeout values read from configuration before defaults are applied.

use std::time::Duration;

/// Holds timeout values parsed from configuration before defaults are applied.
pub(in crate::options) struct TimeoutConfigInput {
    /// Configured connection establishment limit, if present.
    pub(in crate::options) connect_timeout: Option<Duration>,
    /// Configured read-operation limit, if present.
    pub(in crate::options) read_timeout: Option<Duration>,
    /// Configured response-header limit, if present.
    pub(in crate::options) response_header_timeout: Option<Duration>,
    /// Configured whole-request limit, if present.
    pub(in crate::options) request_timeout: Option<Duration>,
}
