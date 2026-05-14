/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use super::{
    SensitiveBodyFields,
    SensitiveHttpHeaders,
    SensitiveQueryParams,
};

/// Policy used by [`LogSanitizer`](super::LogSanitizer) to mask sensitive log data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogSanitizePolicy {
    /// Sensitive HTTP header names.
    pub sensitive_headers: SensitiveHttpHeaders,
    /// Sensitive URL query parameter names.
    pub sensitive_query_params: SensitiveQueryParams,
    /// Sensitive JSON/form body field names.
    pub sensitive_body_fields: SensitiveBodyFields,
}

impl Default for LogSanitizePolicy {
    /// Creates a policy with built-in sensitive header, query, and body names.
    fn default() -> Self {
        Self {
            sensitive_headers: SensitiveHttpHeaders::default(),
            sensitive_query_params: SensitiveQueryParams::default(),
            sensitive_body_fields: SensitiveBodyFields::default(),
        }
    }
}
