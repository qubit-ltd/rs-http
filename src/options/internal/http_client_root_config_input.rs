// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

/// Top-level scalar values read before nested sections and header iteration.
pub(in crate::options) struct HttpClientRootConfigInput {
    /// Optional absolute base URL from the root configuration.
    pub(in crate::options) base_url: Option<String>,
    /// Optional policy governing allowed request origins.
    pub(in crate::options) origin_policy: Option<String>,
    /// Optional switch requesting IPv4-only DNS behavior.
    pub(in crate::options) ipv4_only: Option<bool>,
    /// Optional non-success response preview byte limit.
    pub(in crate::options) error_response_preview_limit: Option<usize>,
    /// Optional retained body byte limit for structured errors.
    pub(in crate::options) error_response_body_limit: Option<usize>,
    /// Optional response body accumulation byte limit.
    pub(in crate::options) response_body_size_limit: Option<usize>,
    /// Optional default user-agent value.
    pub(in crate::options) user_agent: Option<String>,
    /// Optional redirect limit.
    pub(in crate::options) max_redirects: Option<usize>,
    /// Optional connection pool idle timeout.
    pub(in crate::options) pool_idle_timeout: Option<Duration>,
    /// Optional maximum idle connections per host.
    pub(in crate::options) pool_max_idle_per_host: Option<usize>,
    /// Optional environment proxy inheritance switch.
    pub(in crate::options) use_env_proxy: Option<bool>,
}
