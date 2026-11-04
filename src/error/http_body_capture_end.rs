// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Describes how capture of a non-success response body ended.
///
/// Preview truncation is independent: a body whose raw bytes were completely
/// captured has [`Complete`](Self::Complete), even when its preview is shorter.
///
/// # Examples
///
/// ```
/// use qubit_http::{HttpBodyCaptureEnd, HttpClient, HttpError};
///
/// fn inspect(error: &HttpError) {
///     if let Some(response) = error.status_response() {
///         match response.capture_end() {
///             HttpBodyCaptureEnd::Complete => { /* full raw body is available */ }
///             HttpBodyCaptureEnd::SizeLimit
///             | HttpBodyCaptureEnd::ReadTimeout
///             | HttpBodyCaptureEnd::ReadFailure
///             | HttpBodyCaptureEnd::BodyUnavailable => { /* do not parse as complete */ }
///             _ => { /* this enum may gain variants */ }
///         }
///     }
/// }
///
/// let _ = HttpClient::builder();
/// let _ = inspect;
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpBodyCaptureEnd {
    /// Capture reached EOF without omitting raw bytes.
    Complete,
    /// Capture stopped at a configured size bound or omitted raw bytes.
    SizeLimit,
    /// The cumulative timeout for this status-body capture expired.
    ReadTimeout,
    /// The HTTP backend returned an error while reading a body chunk.
    ReadFailure,
    /// The response stream had already been taken and was unavailable.
    BodyUnavailable,
}
