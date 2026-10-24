// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

/// Top-level scalar values read before nested sections and header iteration.
pub(super) struct HttpClientRootConfigInput {
    /// Optional absolute base URL from the root configuration.
    pub(super) base_url: Option<String>,
    /// Optional policy governing allowed request origins.
    pub(super) origin_policy: Option<String>,
    /// Optional switch requesting IPv4-only DNS behavior.
    pub(super) ipv4_only: Option<bool>,
    /// Optional non-success response preview byte limit.
    pub(super) error_response_preview_limit: Option<usize>,
    /// Optional retained body byte limit for structured errors.
    pub(super) error_response_body_limit: Option<usize>,
    /// Optional response body accumulation byte limit.
    pub(super) response_body_size_limit: Option<usize>,
    /// Optional default user-agent value.
    pub(super) user_agent: Option<String>,
    /// Optional redirect limit.
    pub(super) max_redirects: Option<usize>,
    /// Optional connection pool idle timeout.
    pub(super) pool_idle_timeout: Option<Duration>,
    /// Optional maximum idle connections per host.
    pub(super) pool_max_idle_per_host: Option<usize>,
    /// Optional environment proxy inheritance switch.
    pub(super) use_env_proxy: Option<bool>,
}
