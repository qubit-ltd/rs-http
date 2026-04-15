/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # SSE JSON Decoder
//!
//! Converts SSE events into typed JSON chunks.
//!
//! # Author
//!
//! Haixing Hu

use async_stream::stream;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use crate::{HttpByteStream, HttpError};

use super::{decode_events_from_stream_with_limits, DoneMarkerPolicy, SseChunk, SseChunkStream, SseJsonMode};

/// Parses SSE JSON payloads with selectable strictness and explicit line/frame size limits.
///
/// # Parameters
/// - `stream`: SSE HTTP body byte stream.
/// - `done_policy`: Terminal marker detection.
/// - `mode`: Lenient vs strict JSON parsing.
/// - `max_line_bytes`: Maximum allowed bytes for one SSE line.
/// - `max_frame_bytes`: Maximum allowed bytes for one SSE frame.
///
/// # Type parameters
/// - `T`: JSON type to deserialize.
///
/// # Returns
/// Stream of [`SseChunk::Data`] or [`SseChunk::Done`], or errors from upstream/SSE/JSON.
pub(crate) fn decode_json_chunks_from_stream_with_limits<T>(
    stream: HttpByteStream,
    done_policy: DoneMarkerPolicy,
    mode: SseJsonMode,
    max_line_bytes: usize,
    max_frame_bytes: usize,
) -> SseChunkStream<T>
where
    T: DeserializeOwned + Send + 'static,
{
    let mut events = decode_events_from_stream_with_limits(stream, max_line_bytes, max_frame_bytes);

    let output = stream! {
        while let Some(item) = events.next().await {
            let event = match item {
                Ok(event) => event,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            };

            let payload = event.data.trim();
            if payload.is_empty() {
                continue;
            }

            if done_policy.is_done(payload) {
                yield Ok(SseChunk::Done);
                return;
            }

            match serde_json::from_str::<T>(payload) {
                Ok(data) => yield Ok(SseChunk::Data(data)),
                Err(error) => {
                    if mode == SseJsonMode::Lenient {
                        tracing::debug!(
                            "Skipping malformed JSON SSE chunk in lenient mode: {}",
                            error
                        );
                        continue;
                    }

                    yield Err(HttpError::sse_decode(format!(
                        "Failed to decode SSE JSON chunk: {}",
                        error
                    )));
                    return;
                }
            }
        }
    };

    Box::pin(output)
}
