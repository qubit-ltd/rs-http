// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private response implementation state.

mod http_response_body_state;

// Retains cloneable context for repeatable response body-read diagnostics.
pub(super) mod body_read_failure;
// Stores per-response timeout, cancellation, and body-read failure state.
pub(super) mod http_response_runtime;

pub(super) use http_response_body_state::HttpResponseBodyState;
