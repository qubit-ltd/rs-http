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

use super::SseControl;
use super::SseMessage;
use super::SseRecord;
use super::SseRecordStream;
use super::line_decoder::SseLineStream;

/// Groups newline-delimited SSE fields into internal [`SseRecord`] values.
///
/// # Parameters
/// - `lines`: Stream of SSE text lines from
///   [`super::line_decoder::decode_lines`].
/// - `max_frame_bytes`: Maximum allowed bytes for one SSE frame.
///
/// # Returns
/// Stream of records or forwarded transport/protocol errors.
pub(crate) fn decode_records(mut lines: SseLineStream, max_frame_bytes: usize) -> SseRecordStream {
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
                current_frame_bytes = 0;
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
                }
                continue;
            }

            if line.starts_with(':') {
                continue;
            }

            let Some(observed) = current_frame_bytes.checked_add(line.len()) else {
                yield Err(crate::HttpError::sse_protocol(format!(
                    "SSE frame exceeds max_frame_bytes ({})",
                    max_frame_bytes,
                )));
                return;
            };
            if observed > max_frame_bytes {
                yield Err(crate::HttpError::sse_protocol(format!(
                    "SSE frame exceeds max_frame_bytes ({})",
                    max_frame_bytes,
                )));
                return;
            }
            current_frame_bytes = observed;

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

        // Preserve complete control fields at EOF, but never dispatch an
        // event whose terminating blank line was missing.
        if let Some(retry_ms) = current_retry {
            yield Ok(SseRecord::Control(SseControl::ReconnectDelayMs(retry_ms)));
        }
        if !has_data {
            if let Some(id) = current_id {
                yield Ok(SseRecord::Control(SseControl::LastEventId(id)));
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

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures_util::StreamExt;

    use super::decode_records;
    use crate::HttpByteStream;
    use crate::sse::SseControl;
    use crate::sse::SseRecord;
    use crate::sse::line_decoder::decode_lines;

    async fn decode(chunks: Vec<&'static [u8]>) -> crate::HttpResult<Vec<SseRecord>> {
        decode_with_limit(chunks, 1024).await
    }

    async fn decode_with_limit(
        chunks: Vec<&'static [u8]>,
        max_frame_bytes: usize,
    ) -> crate::HttpResult<Vec<SseRecord>> {
        let chunks = chunks.into_iter().map(Bytes::from_static).map(Ok);
        let input: HttpByteStream = Box::pin(futures_util::stream::iter(chunks));
        let lines = decode_lines(input, 64);
        decode_records(lines, max_frame_bytes)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect()
    }

    #[tokio::test]
    async fn test_decode_records_preserves_control_fields_from_eof_fragment() {
        let records = decode(vec![b"id: resume\nretry: 17"]).await.unwrap();
        assert_eq!(
            records,
            vec![
                SseRecord::Control(SseControl::ReconnectDelayMs(17)),
                SseRecord::Control(SseControl::LastEventId("resume".to_owned())),
            ]
        );

        let records = decode(vec![b"id: pending\ndata: incomplete"]).await.unwrap();
        assert!(records.is_empty());
    }

    #[tokio::test]
    async fn test_unknown_fields_do_not_carry_frame_budget_across_blank_lines() {
        let records = decode_with_limit(vec![b"x: 123\n\n", b"x: 123\n\n", b"data: a\n\n"], 8)
            .await
            .expect("each blank line resets the frame byte budget");
        assert_eq!(
            records,
            vec![SseRecord::Dispatch(crate::sse::SseMessage {
                event: None,
                data: "a".to_owned(),
                last_event_id: None,
            })]
        );
    }
}
