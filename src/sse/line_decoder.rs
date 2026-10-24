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

/// Decodes the buffered bytes as UTF-8 and clears the buffer, including when
/// decoding fails.
///
/// # Parameters
/// - `buffer`: Current line bytes without the line terminator.
///
/// # Returns
/// The decoded UTF-8 line text.
///
/// # Errors
/// Returns an SSE protocol error when the buffered bytes are not valid UTF-8.
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
/// A stream yielding decoded lines or errors. Input stream errors are
/// forwarded; invalid UTF-8 and lines larger than `max_line_bytes` yield SSE
/// protocol errors and terminate the output stream.
#[must_use]
pub fn decode_lines(mut stream: HttpByteStream, max_line_bytes: usize) -> SseLineStream {
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
/// - `max_line_bytes`: Inclusive size limit for one line.
///
/// # Returns
/// `Ok(())` when appended.
///
/// # Errors
/// Returns an SSE protocol error if appending would exceed `max_line_bytes` or
/// if calculating the resulting length overflows `usize`.
fn append_line_bytes(buffer: &mut BytesMut, bytes: &[u8], max_line_bytes: usize) -> HttpResult<()> {
    let Some(observed) = buffer.len().checked_add(bytes.len()) else {
        return Err(HttpError::sse_protocol(format!(
            "SSE line exceeds max_line_bytes ({})",
            max_line_bytes,
        )));
    };
    if observed > max_line_bytes {
        return Err(HttpError::sse_protocol(format!(
            "SSE line exceeds max_line_bytes ({})",
            max_line_bytes,
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
#[inline]
fn is_line_terminator(byte: u8) -> bool {
    matches!(byte, b'\r' | b'\n')
}
