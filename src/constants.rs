/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Crate-wide defaults and fixed tuning values for HTTP client behavior and logging.

pub use crate::sanitize::{
    DEFAULT_SENSITIVE_BODY_FIELD_NAMES,
    DEFAULT_SENSITIVE_HEADER_NAMES,
    DEFAULT_SENSITIVE_QUERY_PARAM_NAMES,
};

// ---------------------------------------------------------------------------
// Timeouts ([`crate::HttpTimeoutOptions::default`])
// ---------------------------------------------------------------------------

/// Default connect timeout in seconds.
pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Default read timeout in seconds.
pub const DEFAULT_READ_TIMEOUT_SECS: u64 = 120;

/// Default write timeout in seconds.
pub const DEFAULT_WRITE_TIMEOUT_SECS: u64 = 120;

// ---------------------------------------------------------------------------
// Logging ([`crate::HttpLoggingOptions::default`])
// ---------------------------------------------------------------------------

/// Default maximum body bytes included in TRACE log previews.
pub const DEFAULT_LOG_BODY_SIZE_LIMIT_BYTES: usize = 16 * 1024;

/// Default maximum bytes included in non-success response body previews on [`crate::HttpError`].
pub const DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES: usize = 16 * 1024;

// ---------------------------------------------------------------------------
// SSE decode safety limits
// ---------------------------------------------------------------------------

/// Default maximum bytes allowed for a single SSE line before raising a protocol error.
pub const DEFAULT_SSE_MAX_LINE_BYTES: usize = 64 * 1024;

/// Default maximum bytes allowed for one SSE frame (between blank lines) before raising a protocol error.
pub const DEFAULT_SSE_MAX_FRAME_BYTES: usize = 1024 * 1024;
