// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Decode and error-preview options bound to one response instance.

use qubit_budget::json::JsonValueLimits;
use qubit_redact::Redactor;

use crate::constants::DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES;
use crate::constants::DEFAULT_RESPONSE_BODY_SIZE_LIMIT_BYTES;
use crate::constants::DEFAULT_SSE_MAX_FRAME_BYTES;
use crate::constants::DEFAULT_SSE_MAX_LINE_BYTES;
use crate::json_limits::default_json_value_limits;
use crate::options::HttpClientOptions;
use crate::sse::DoneMarkerPolicy;
use crate::sse::SseJsonMode;
/// Decode/error-preview options bound to one response instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpResponseOptions {
    /// Maximum bytes captured for status-error body preview.
    pub error_response_preview_limit: usize,
    /// Maximum bytes accumulated by whole-body response readers.
    pub response_body_size_limit: usize,
    /// Structural and decoded-payload limits for JSON bodies and SSE data.
    pub json_value_limits: JsonValueLimits,
    /// Default JSON decoding mode used by stream JSON helpers.
    pub sse_json_mode: SseJsonMode,
    /// Default maximum bytes allowed for one SSE line.
    pub sse_max_line_bytes: usize,
    /// Default maximum bytes allowed for one SSE frame.
    pub sse_max_frame_bytes: usize,
    /// How [`crate::HttpResponse::sse_chunks`] recognizes end-of-stream
    /// `data:` markers.
    pub sse_done_marker_policy: DoneMarkerPolicy,
    /// Shared redactor used for status-error body previews.
    pub log_redactor: Redactor,
}

impl Default for HttpResponseOptions {
    fn default() -> Self {
        Self {
            error_response_preview_limit: DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES,
            response_body_size_limit: DEFAULT_RESPONSE_BODY_SIZE_LIMIT_BYTES,
            json_value_limits: default_json_value_limits(),
            sse_json_mode: SseJsonMode::Lenient,
            sse_max_line_bytes: DEFAULT_SSE_MAX_LINE_BYTES,
            sse_max_frame_bytes: DEFAULT_SSE_MAX_FRAME_BYTES,
            sse_done_marker_policy: DoneMarkerPolicy::default(),
            log_redactor: Redactor::application_default(),
        }
    }
}

impl HttpResponseOptions {
    /// Captures response decoding options from one client configuration.
    ///
    /// # Parameters
    ///
    /// * `options` - Client options whose current values are snapshotted.
    /// * `log_redactor` - Request-specific redactor retained by the response.
    ///
    /// # Returns
    ///
    /// An independent response option snapshot.
    pub(crate) fn from_client_options(options: &HttpClientOptions, log_redactor: Redactor) -> Self {
        Self {
            error_response_preview_limit: options.error_response_preview_limit.max(1),
            response_body_size_limit: options.response_body_size_limit.max(1),
            json_value_limits: options.json_value_limits,
            sse_json_mode: options.sse_json_mode,
            sse_max_line_bytes: options.sse_max_line_bytes.max(1),
            sse_max_frame_bytes: options.sse_max_frame_bytes.max(1),
            sse_done_marker_policy: options.sse_done_marker_policy.clone(),
            log_redactor,
        }
    }
}
