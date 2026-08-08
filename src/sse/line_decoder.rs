// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # SSE Line Decoder
//!
//! Splits byte stream into text lines.

use std::pin::Pin;

use async_stream::stream;
use bytes::BytesMut;
use futures_util::Stream;
use futures_util::StreamExt;

use crate::HttpByteStream;
use crate::HttpError;
use crate::HttpResult;

/// Pin-boxed stream of newline-delimited text lines or errors.
pub type SseLineStream = Pin<Box<dyn Stream<Item = HttpResult<String>> + Send>>;

/// Decodes and clears the currently buffered line bytes.
///
/// # Parameters
/// - `buffer`: Current line bytes without the line terminator.
///
/// # Returns
/// UTF-8 line text.
fn take_buffered_line(buffer: &mut BytesMut) -> HttpResult<String> {
    let decoded = match std::str::from_utf8(buffer.as_ref()) {
        Ok(text) => Ok(text.to_string()),
        Err(error) => Err(HttpError::sse_protocol(format!(
            "Failed to decode SSE line as UTF-8: {error}"
        ))),
    };
    buffer.clear();
    decoded
}

/// Buffers chunks from `stream`, splits on LF, CR, or CRLF, and validates UTF-8
/// per line.
///
/// # Parameters
/// - `stream`: Raw byte stream from [`crate::HttpResponse::stream`].
/// - `max_line_bytes`: Maximum allowed bytes for one SSE line.
///
/// # Returns
/// Stream of lines or [`HttpError::sse_protocol`] on invalid UTF-8.
pub fn decode_lines(
    mut stream: HttpByteStream,
    max_line_bytes: usize,
) -> SseLineStream {
    let output = stream! {
        let max_line_bytes = max_line_bytes.max(1);
        let mut buffer = BytesMut::new();
        let mut pending_cr = false;

        while let Some(item) = stream.next().await {
            match item {
                Ok(chunk) => {
                    let mut remaining = chunk.as_ref();
                    while !remaining.is_empty() {
                        let Some(index) = remaining.iter().position(|byte| is_line_terminator(*byte)) else {
                            pending_cr = false;
                            if let Err(error) =
                                append_line_bytes(&mut buffer, remaining, max_line_bytes)
                            {
                                yield Err(error);
                                return;
                            }
                            break;
                        };

                        let segment = &remaining[..index];
                        if !segment.is_empty() {
                            pending_cr = false;
                        }
                        if let Err(error) = append_line_bytes(&mut buffer, segment, max_line_bytes) {
                            yield Err(error);
                            return;
                        }

                        let terminator = remaining[index];
                        remaining = &remaining[index + 1..];
                        if terminator == b'\n' && pending_cr {
                            pending_cr = false;
                            continue;
                        }

                        match take_buffered_line(&mut buffer) {
                            Ok(text) => yield Ok(text),
                            Err(error) => {
                                yield Err(error);
                                return;
                            }
                        }
                        pending_cr = terminator == b'\r';
                    }
                }
                Err(error) => {
                    yield Err(error);
                    return;
                }
            }
        }

        if !buffer.is_empty() {
            match take_buffered_line(&mut buffer) {
                Ok(text) => yield Ok(text),
                Err(error) => yield Err(error),
            }
        }
    };

    Box::pin(output)
}

/// Appends one contiguous line segment and enforces the configured byte limit.
///
/// # Parameters
/// - `buffer`: Current line buffer.
/// - `bytes`: Bytes before the next line terminator.
/// - `max_line_bytes`: Maximum allowed bytes for one line.
///
/// # Returns
/// `Ok(())` when appended, or [`HttpError`] when the line is too large.
fn append_line_bytes(
    buffer: &mut BytesMut,
    bytes: &[u8],
    max_line_bytes: usize,
) -> HttpResult<()> {
    if buffer.len() + bytes.len() > max_line_bytes {
        return Err(HttpError::sse_protocol(format!(
            "SSE line exceeds max_line_bytes ({max_line_bytes})"
        )));
    }
    buffer.extend_from_slice(bytes);
    Ok(())
}

/// Returns whether a byte terminates one SSE line.
///
/// # Parameters
/// - `byte`: Byte to test.
///
/// # Returns
/// `true` for CR or LF.
fn is_line_terminator(byte: u8) -> bool {
    matches!(byte, b'\r' | b'\n')
}
