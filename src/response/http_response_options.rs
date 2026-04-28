/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Decode and error-preview options bound to one response instance.

use crate::constants::{
    DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES, DEFAULT_SSE_MAX_FRAME_BYTES,
    DEFAULT_SSE_MAX_LINE_BYTES,
};
use crate::sse::{DoneMarkerPolicy, SseJsonMode};

/// Decode/error-preview options bound to one response instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpResponseOptions {
    /// Maximum bytes captured for status-error body preview.
    pub error_response_preview_limit: usize,
    /// Default JSON decoding mode used by stream JSON helpers.
    pub sse_json_mode: SseJsonMode,
    /// Default maximum bytes allowed for one SSE line.
    pub sse_max_line_bytes: usize,
    /// Default maximum bytes allowed for one SSE frame.
    pub sse_max_frame_bytes: usize,
    /// How [`crate::HttpResponse::sse_chunks`] recognizes end-of-stream `data:` markers.
    pub sse_done_marker_policy: DoneMarkerPolicy,
}

impl Default for HttpResponseOptions {
    fn default() -> Self {
        Self {
            error_response_preview_limit: DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES,
            sse_json_mode: SseJsonMode::Lenient,
            sse_max_line_bytes: DEFAULT_SSE_MAX_LINE_BYTES,
            sse_max_frame_bytes: DEFAULT_SSE_MAX_FRAME_BYTES,
            sse_done_marker_policy: DoneMarkerPolicy::default(),
        }
    }
}

impl HttpResponseOptions {
    pub(crate) fn new(
        error_response_preview_limit: usize,
        sse_json_mode: SseJsonMode,
        sse_max_line_bytes: usize,
        sse_max_frame_bytes: usize,
        sse_done_marker_policy: DoneMarkerPolicy,
    ) -> Self {
        Self {
            error_response_preview_limit: error_response_preview_limit.max(1),
            sse_json_mode,
            sse_max_line_bytes: sse_max_line_bytes.max(1),
            sse_max_frame_bytes: sse_max_frame_bytes.max(1),
            sse_done_marker_policy,
        }
    }
}
