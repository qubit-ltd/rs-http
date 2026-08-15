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

pub(in crate::client) use http_attempt_execution_context::HttpAttemptExecutionContext;
pub(in crate::client) use http_attempt_response::HttpAttemptResponse;
