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
//! SSE utilities built on top of [`HttpByteStream`](crate::HttpByteStream).
//!
//! # Author
//!
//! Haixing Hu

mod done_marker_policy;
mod event_decoder;
mod frame_decoder;
mod json_decoder;
mod line_decoder;
mod sse_chunk;
mod sse_chunk_stream;
mod sse_event;
mod sse_event_stream;
mod sse_json_mode;
mod sse_reconnect_options;
mod sse_reconnect_runner;

pub(crate) use event_decoder::decode_events_from_stream_with_limits;
pub(crate) use sse_reconnect_runner::SseReconnectRunner;

pub use done_marker_policy::DoneMarkerPolicy;
pub use sse_chunk::SseChunk;
pub use sse_chunk_stream::SseChunkStream;
pub use sse_event::SseEvent;
pub use sse_event_stream::SseEventStream;
pub use sse_json_mode::SseJsonMode;
pub use sse_reconnect_options::SseReconnectOptions;

pub(crate) use json_decoder::decode_json_chunks_from_stream_with_limits;
pub(crate) use sse_reconnect_options::DEFAULT_SSE_MAX_RECONNECT_DELAY;

#[cfg(coverage)]
#[doc(hidden)]
pub(crate) use sse_reconnect_runner::coverage_validate_sse_response_content_type;
