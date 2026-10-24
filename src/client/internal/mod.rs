// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private execution state owned by the HTTP client pipeline.

mod http_attempt_execution_context;
mod http_attempt_response;
mod http_retry_application_source;
mod http_retry_rule;
mod ipv4_only_resolver;
mod redirect_origin_violation;

pub(in crate::client) use http_attempt_execution_context::HttpAttemptExecutionContext;
pub(in crate::client) use http_attempt_response::HttpAttemptResponse;
pub(in crate::client) use http_retry_application_source::HttpRetryApplicationSource;
pub(in crate::client) use http_retry_rule::HttpRetryRule;
pub(super) use ipv4_only_resolver::Ipv4OnlyResolver;
pub(super) use redirect_origin_violation::RedirectOriginViolation;
