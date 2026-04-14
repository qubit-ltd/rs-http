/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! SSE decode options attached to streaming responses.

use crate::{
    constants::{DEFAULT_SSE_MAX_FRAME_BYTES, DEFAULT_SSE_MAX_LINE_BYTES},
    sse::SseJsonMode,
};

/// SSE decode options attached to one streaming response body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SseDecodeOptions {
    /// Default JSON decoding mode used by stream JSON helpers.
    pub json_mode: SseJsonMode,
    /// Default maximum bytes allowed for one SSE line.
    pub max_line_bytes: usize,
    /// Default maximum bytes allowed for one SSE frame.
    pub max_frame_bytes: usize,
}

impl Default for SseDecodeOptions {
    /// Builds default SSE decode options.
    fn default() -> Self {
        Self {
            json_mode: SseJsonMode::Lenient,
            max_line_bytes: DEFAULT_SSE_MAX_LINE_BYTES,
            max_frame_bytes: DEFAULT_SSE_MAX_FRAME_BYTES,
        }
    }
}

impl SseDecodeOptions {
    /// Creates options and normalizes line/frame limits to at least 1 byte.
    pub fn new(json_mode: SseJsonMode, max_line_bytes: usize, max_frame_bytes: usize) -> Self {
        Self {
            json_mode,
            max_line_bytes: max_line_bytes.max(1),
            max_frame_bytes: max_frame_bytes.max(1),
        }
    }
}
