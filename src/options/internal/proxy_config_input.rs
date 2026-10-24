// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Raw proxy values read before defaults and semantic validation are applied.

/// Intermediate proxy configuration before defaults and validation.
pub(in crate::options) struct ProxyConfigInput {
    /// Configured enablement override, if present.
    pub(in crate::options) enabled: Option<bool>,
    /// Configured proxy type spelling, if present.
    pub(in crate::options) proxy_type: Option<String>,
    /// Configured proxy host, if present.
    pub(in crate::options) host: Option<String>,
    /// Configured proxy port, if present.
    pub(in crate::options) port: Option<u16>,
    /// Configured authentication username, if present.
    pub(in crate::options) username: Option<String>,
    /// Configured authentication password, if present.
    pub(in crate::options) password: Option<String>,
}
