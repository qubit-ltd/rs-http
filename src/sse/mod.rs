/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # SSE Decoding
//!
//! SSE utilities built on top of [`HttpStreamResponse`](crate::HttpStreamResponse).
//!
//! # Author
//!
//! Haixing Hu

use std::time::Duration;

mod done_marker_policy;
mod frame_decoder;
mod json_decoder;
mod line_decoder;
mod sse_chunk;
mod sse_chunk_stream;
mod sse_event;
mod sse_event_stream;
mod sse_json_mode;

use crate::HttpStreamResponse;

pub use done_marker_policy::DoneMarkerPolicy;
pub use sse_chunk::SseChunk;
pub use sse_chunk_stream::SseChunkStream;
pub use sse_event::SseEvent;
pub use sse_event_stream::SseEventStream;
pub use sse_json_mode::SseJsonMode;

pub(crate) use json_decoder::decode_json_chunks_from_response_with_limits;

/// Reconnect behavior options for [`crate::HttpClient::execute_sse_with_reconnect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseReconnectOptions {
    /// Maximum reconnect attempts after the first connection.
    pub max_reconnects: u32,
    /// Base reconnect delay between attempts.
    pub reconnect_delay: Duration,
    /// Whether to reconnect when the SSE stream ends without an explicit error.
    pub reconnect_on_eof: bool,
    /// Whether to honor SSE `retry:` field as the next reconnect delay.
    pub honor_server_retry: bool,
}

impl Default for SseReconnectOptions {
    /// Builds default SSE reconnect options.
    ///
    /// # Returns
    /// Default reconnect options with bounded reconnect attempts.
    fn default() -> Self {
        Self {
            max_reconnects: 3,
            reconnect_delay: Duration::from_secs(1),
            reconnect_on_eof: true,
            honor_server_retry: true,
        }
    }
}

impl SseReconnectOptions {
    /// Creates default SSE reconnect options.
    ///
    /// # Returns
    /// Same as [`SseReconnectOptions::default`].
    pub fn new() -> Self {
        Self::default()
    }
}

/// Parses SSE frames from a streaming HTTP response with explicit line/frame size limits.
///
/// # Parameters
/// - `stream`: Streaming response whose body is SSE text.
/// - `max_line_bytes`: Maximum allowed bytes for one SSE line.
/// - `max_frame_bytes`: Maximum allowed bytes for one SSE frame.
///
/// # Returns
/// Stream yielding [`SseEvent`] values or protocol/transport errors.
pub(crate) fn decode_events_from_response_with_limits(
    stream: HttpStreamResponse,
    max_line_bytes: usize,
    max_frame_bytes: usize,
) -> SseEventStream {
    let lines = line_decoder::decode_lines(stream.into_stream(), max_line_bytes);
    frame_decoder::decode_frames(lines, max_frame_bytes)
}
