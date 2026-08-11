// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Decode public SSE messages from an HTTP body byte stream with size limits.

use async_stream::stream;
use futures_util::StreamExt;

use super::SseMessageStream;
use super::SseRecord;
use super::SseRecordStream;
use super::frame_decoder;
use super::line_decoder;
use crate::HttpByteStream;

/// Parses internal SSE records from a body byte stream with explicit size
/// limits.
///
/// # Parameters
/// - `stream`: Body byte stream whose payload is SSE text.
/// - `max_line_bytes`: Maximum allowed bytes for one SSE line.
/// - `max_frame_bytes`: Maximum allowed bytes for one SSE frame.
///
/// # Returns
/// Stream yielding internal [`SseRecord`] values or protocol/transport errors.
pub(crate) fn decode_records_from_stream_with_limits(
    stream: HttpByteStream,
    max_line_bytes: usize,
    max_frame_bytes: usize,
) -> SseRecordStream {
    let lines = line_decoder::decode_lines(stream, max_line_bytes);
    frame_decoder::decode_records(lines, max_frame_bytes)
}

/// Parses public SSE messages from a body byte stream with explicit size
/// limits.
///
/// # Parameters
/// - `stream`: Body byte stream whose payload is SSE text.
/// - `max_line_bytes`: Maximum allowed bytes for one SSE line.
/// - `max_frame_bytes`: Maximum allowed bytes for one SSE frame.
///
/// # Returns
/// Stream yielding [`super::SseMessage`] values or protocol/transport errors.
pub(crate) fn decode_messages_from_stream_with_limits(
    stream: HttpByteStream,
    max_line_bytes: usize,
    max_frame_bytes: usize,
) -> SseMessageStream {
    let mut records = decode_records_from_stream_with_limits(
        stream,
        max_line_bytes,
        max_frame_bytes,
    );
    let output = stream! {
        while let Some(item) = records.next().await {
            match item {
                Ok(SseRecord::Dispatch(message)) => yield Ok(message),
                Ok(SseRecord::Control(_)) => continue,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            }
        }
    };
    Box::pin(output)
}
