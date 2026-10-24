// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Optional values read from the HTTP logging configuration section.

/// Holds logging values before they are merged into their defaults.
pub(in crate::options) struct HttpLoggingConfigInput {
    /// Whether TRACE-level logging is enabled, if configured.
    pub(in crate::options) enabled: Option<bool>,
    /// Whether request headers are logged, if configured.
    pub(in crate::options) log_request_header: Option<bool>,
    /// Whether request bodies are logged, if configured.
    pub(in crate::options) log_request_body: Option<bool>,
    /// Whether response headers are logged, if configured.
    pub(in crate::options) log_response_header: Option<bool>,
    /// Whether response bodies are logged, if configured.
    pub(in crate::options) log_response_body: Option<bool>,
    /// Maximum body preview size in bytes, if configured.
    pub(in crate::options) body_size_limit: Option<usize>,
}
