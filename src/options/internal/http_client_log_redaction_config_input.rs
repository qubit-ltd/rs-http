// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Parsed values from the `log_redaction` configuration section.

/// Intermediate log-redaction configuration before policy validation.
pub(in crate::options) struct HttpClientLogRedactionConfigInput {
    /// Optional URL path visibility setting.
    pub(in crate::options) url_path_policy: Option<String>,
    /// Header names whose sensitivity should be raised.
    pub(in crate::options) sensitive_headers: Option<Vec<String>>,
    /// Query parameter names whose sensitivity should be raised.
    pub(in crate::options) sensitive_query_params: Option<Vec<String>>,
    /// Structured body field names whose sensitivity should be raised.
    pub(in crate::options) sensitive_body_fields: Option<Vec<String>>,
    /// Exact sensitive header names explicitly allowed to remain visible.
    pub(in crate::options) excluded_sensitive_headers: Option<Vec<String>>,
    /// Exact sensitive query names explicitly allowed to remain visible.
    pub(in crate::options) excluded_sensitive_query_params: Option<Vec<String>>,
    /// Exact sensitive body field names explicitly allowed to remain visible.
    pub(in crate::options) excluded_sensitive_body_fields: Option<Vec<String>>,
}
