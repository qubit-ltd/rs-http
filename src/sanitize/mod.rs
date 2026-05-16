/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Log sanitization policies and helpers.

mod body_log_context;
mod body_preview;
mod default_sensitive_names;
mod log_sanitize_policy;
mod log_sanitizer;
mod sanitized_debugger;
mod sensitive_body_fields;
mod sensitive_http_headers;
mod sensitive_query_params;

pub use body_log_context::BodyLogContext;
pub use body_preview::BodyPreview;
pub use default_sensitive_names::{
    DEFAULT_SENSITIVE_BODY_FIELD_NAMES,
    DEFAULT_SENSITIVE_HEADER_NAMES,
    DEFAULT_SENSITIVE_QUERY_PARAM_NAMES,
};
pub use log_sanitize_policy::LogSanitizePolicy;
pub use log_sanitizer::LogSanitizer;
pub(crate) use sanitized_debugger::SanitizedDebugger;
pub use sensitive_body_fields::SensitiveBodyFields;
pub use sensitive_http_headers::SensitiveHttpHeaders;
pub use sensitive_query_params::SensitiveQueryParams;
