/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Crate-wide defaults and fixed tuning values for HTTP client behavior and logging.

// ---------------------------------------------------------------------------
// Sensitive headers (log masking)
// ---------------------------------------------------------------------------

/// Built-in header names preloaded into [`crate::SensitiveHeaders::default`] for log masking.
pub const DEFAULT_SENSITIVE_HEADER_NAMES: [&str; 20] = [
    "Authorization",
    "Api-Key",
    "X-Api-Key",
    "Bearer",
    "Cookie",
    "Set-Cookie",
    "Secret-Key",
    "Client-Secret",
    "Access-Token",
    "Refresh-Token",
    "Private-Token",
    "Session-Token",
    "JWT-Token",
    "Password",
    "X-Auth-Password",
    "X-Client-ID",
    "X-Client-Secret",
    "X-Auth-Token",
    "X-Auth-App-Token",
    "X-Auth-User-Token",
];

// ---------------------------------------------------------------------------
// Timeouts ([`crate::TimeoutOptions::default`])
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

// ---------------------------------------------------------------------------
// Sensitive header value masking rules used by [`crate::HttpLogger`]
// ---------------------------------------------------------------------------

/// Values with at most this many characters are fully replaced by [`SENSITIVE_HEADER_MASK_PLACEHOLDER`].
pub const SENSITIVE_HEADER_MASK_SHORT_LEN: usize = 4;

/// How many characters to keep visible at the start and end when masking longer values.
pub const SENSITIVE_HEADER_MASK_EDGE_CHARS: usize = 2;

/// Replacement string for fully masked or middle segments of sensitive header values.
pub const SENSITIVE_HEADER_MASK_PLACEHOLDER: &str = "****";
