// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # SSE Frame Decoder
//!
//! Builds structured SSE records from line stream.

use async_stream::stream;
use futures_util::StreamExt;

use super::line_decoder::SseLineStream;
use super::{
    SseControl,
    SseMessage,
    SseRecord,
    SseRecordStream,
};

/// Groups newline-delimited SSE fields into internal [`SseRecord`] values.
///
/// # Parameters
/// - `lines`: Stream of SSE text lines from
///   [`super::line_decoder::decode_lines`].
/// - `max_frame_bytes`: Maximum allowed bytes for one SSE frame.
///
/// # Returns
/// Stream of records or forwarded transport/protocol errors.
pub(crate) fn decode_records(
    mut lines: SseLineStream,
    max_frame_bytes: usize,
) -> SseRecordStream {
    let output = stream! {
        let max_frame_bytes = max_frame_bytes.max(1);
        let mut current_event: Option<String> = None;
        let mut current_id: Option<String> = None;
        let mut current_retry: Option<u64> = None;
        let mut current_data: Vec<String> = Vec::new();
        let mut has_fields = false;
        let mut has_data = false;
        let mut current_frame_bytes = 0usize;
        let mut last_event_id: Option<String> = None;

        while let Some(item) = lines.next().await {
            let line = match item {
                Ok(line) => line,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            };

            if line.is_empty() {
                if has_fields {
                    if let Some(retry_ms) = current_retry.take() {
                        yield Ok(SseRecord::Control(SseControl::ReconnectDelayMs(retry_ms)));
                    }
                    if let Some(id) = current_id.take() {
                        last_event_id = Some(id.clone());
                        if !has_data {
                            yield Ok(SseRecord::Control(SseControl::LastEventId(id)));
                        }
                    }
                    if has_data {
                        yield Ok(SseRecord::Dispatch(SseMessage {
                            event: current_event.take(),
                            data: current_data.join("\n"),
                            last_event_id: last_event_id.clone(),
                        }));
                    } else {
                        current_event.take();
                    }
                    current_data.clear();
                    has_fields = false;
                    has_data = false;
                    current_frame_bytes = 0;
                }
                continue;
            }

            if line.starts_with(':') {
                continue;
            }

            current_frame_bytes += line.len();
            if current_frame_bytes > max_frame_bytes {
                yield Err(crate::HttpError::sse_protocol(format!(
                    "SSE frame exceeds max_frame_bytes ({max_frame_bytes})"
                )));
                return;
            }

            let (field, value) = split_field_value(&line);
            match field {
                "event" => {
                    current_event = Some(value.to_string());
                    has_fields = true;
                }
                "data" => {
                    current_data.push(value.to_string());
                    has_fields = true;
                    has_data = true;
                }
                "id" => {
                    if !value.contains('\0') {
                        current_id = Some(value.to_string());
                        has_fields = true;
                    }
                }
                "retry" => {
                    if let Ok(retry) = value.parse::<u64>() {
                        current_retry = Some(retry);
                    }
                    has_fields = true;
                }
                _ => {}
            }
        }

        if has_fields {
            if let Some(retry_ms) = current_retry {
                yield Ok(SseRecord::Control(SseControl::ReconnectDelayMs(retry_ms)));
            }
            if let Some(id) = current_id {
                last_event_id = Some(id.clone());
                if !has_data {
                    yield Ok(SseRecord::Control(SseControl::LastEventId(id)));
                }
            }
            if has_data {
                yield Ok(SseRecord::Dispatch(SseMessage {
                    event: current_event,
                    data: current_data.join("\n"),
                    last_event_id,
                }));
            }
        }
    };

    Box::pin(output)
}

/// Splits `line` into field name and value at the first colon (SSE `field:
/// value` syntax).
///
/// # Parameters
/// - `line`: One non-empty SSE line (comments already filtered).
///
/// # Returns
/// `(field, value)`; value has a single leading space stripped if present; if
/// no colon, `(line, "")`.
fn split_field_value(line: &str) -> (&str, &str) {
    if let Some(index) = line.find(':') {
        let field = &line[..index];
        let mut value = &line[index + 1..];
        if value.starts_with(' ') {
            value = &value[1..];
        }
        (field, value)
    } else {
        (line, "")
    }
}
