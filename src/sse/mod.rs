// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # SSE Decoding
//!
//! SSE utilities built on top of [`HttpByteStream`](crate::HttpByteStream).

mod done_marker_policy;
mod frame_decoder;
mod json_decoder;
mod line_decoder;
mod message_decoder;
mod sse_chunk;
mod sse_chunk_stream;
mod sse_control;
mod sse_json_mode;
mod sse_message;
mod sse_message_stream;
mod sse_reconnect_options;
mod sse_reconnect_runner;
mod sse_record;
mod sse_record_stream;

pub use done_marker_policy::DoneMarkerPolicy;
pub(crate) use json_decoder::decode_json_chunks_from_stream_with_limits;
pub(crate) use message_decoder::decode_messages_from_stream_with_limits;
pub(crate) use message_decoder::decode_records_from_stream_with_limits;
pub use sse_chunk::SseChunk;
pub use sse_chunk_stream::SseChunkStream;
pub(crate) use sse_control::SseControl;
pub use sse_json_mode::SseJsonMode;
pub use sse_message::SseMessage;
pub use sse_message_stream::SseMessageStream;
pub use sse_reconnect_options::SseReconnectOptions;
pub(crate) use sse_reconnect_options::DEFAULT_SSE_MAX_RECONNECT_DELAY;
pub(crate) use sse_reconnect_runner::SseReconnectRunner;
pub(crate) use sse_record::SseRecord;
pub(crate) use sse_record_stream::SseRecordStream;
