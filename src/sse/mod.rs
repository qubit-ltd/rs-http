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

pub(crate) use json_decoder::decode_json_chunks_from_response;

/// Parses SSE frames from a streaming HTTP response (UTF-8 lines → events).
///
/// # Parameters
/// - `stream`: Streaming response whose body is SSE text.
///
/// # Returns
/// Stream yielding [`SseEvent`] values or protocol/transport errors.
pub(crate) fn decode_events_from_response(stream: HttpStreamResponse) -> SseEventStream {
    let lines = line_decoder::decode_lines(stream.into_stream());
    frame_decoder::decode_frames(lines)
}
