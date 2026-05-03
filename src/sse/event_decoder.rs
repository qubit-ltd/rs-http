/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Decode SSE events from an HTTP body byte stream with size limits.
//!

use crate::HttpByteStream;

use super::{
    frame_decoder,
    line_decoder,
    SseEventStream,
};

/// Parses SSE frames from a body byte stream with explicit line/frame size limits.
///
/// # Parameters
/// - `stream`: Body byte stream whose payload is SSE text.
/// - `max_line_bytes`: Maximum allowed bytes for one SSE line.
/// - `max_frame_bytes`: Maximum allowed bytes for one SSE frame.
///
/// # Returns
/// Stream yielding [`super::SseEvent`] values or protocol/transport errors.
pub(crate) fn decode_events_from_stream_with_limits(
    stream: HttpByteStream,
    max_line_bytes: usize,
    max_frame_bytes: usize,
) -> SseEventStream {
    let lines = line_decoder::decode_lines(stream, max_line_bytes);
    frame_decoder::decode_frames(lines, max_frame_bytes)
}
