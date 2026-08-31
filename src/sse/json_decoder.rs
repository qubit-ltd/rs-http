// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # SSE JSON Decoder
//!
//! Converts SSE events into typed JSON chunks.

use async_stream::stream;
use futures_util::StreamExt;
use qubit_budget::json::JsonValueLimits;
use serde::de::DeserializeOwned;

use super::DoneMarkerPolicy;
use super::SseChunk;
use super::SseChunkStream;
use super::SseJsonMode;
use super::decode_messages_from_stream_with_limits;
use crate::HttpByteStream;
use crate::json_limits::json_decode_limits;

/// Parses SSE JSON payloads with selectable strictness and explicit line/frame
/// size limits.
///
/// # Parameters
/// - `stream`: SSE HTTP body byte stream.
/// - `done_policy`: Terminal marker detection.
/// - `mode`: Lenient vs strict JSON parsing.
/// - `max_line_bytes`: Maximum allowed bytes for one SSE line.
/// - `max_frame_bytes`: Maximum allowed bytes for one SSE frame.
/// - `json_value_limits`: Structural and decoded-payload limits for each JSON
///   event.
///
/// # Type parameters
/// - `T`: JSON type to deserialize.
///
/// # Returns
/// Stream of [`SseChunk::Data`] or [`SseChunk::Done`], or errors from
/// upstream/SSE/JSON.
pub(crate) fn decode_json_chunks_from_stream_with_limits<T>(
    stream: HttpByteStream,
    done_policy: DoneMarkerPolicy,
    mode: SseJsonMode,
    max_line_bytes: usize,
    max_frame_bytes: usize,
    json_value_limits: JsonValueLimits,
) -> SseChunkStream<T>
where
    T: DeserializeOwned + Send + 'static,
{
    let mut messages = decode_messages_from_stream_with_limits(stream, max_line_bytes, max_frame_bytes);

    let output = stream! {
        while let Some(item) = messages.next().await {
            let message = match item {
                Ok(message) => message,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            };

            let payload = message.data.trim();
            if payload.is_empty() {
                continue;
            }

            if done_policy.is_done(payload) {
                yield Ok(SseChunk::Done);
                return;
            }

            let limits =
                json_decode_limits(max_frame_bytes, json_value_limits);
            match message.decode_json_with_mode_and_limits::<T>(mode, limits) {
                Ok(Some(data)) => yield Ok(SseChunk::Data(data)),
                Ok(None) => continue,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            }
        }
    };

    Box::pin(output)
}
