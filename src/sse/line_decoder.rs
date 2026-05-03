/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! # SSE Line Decoder
//!
//! Splits byte stream into text lines.
//!

use std::pin::Pin;

use async_stream::stream;
use bytes::BytesMut;
use futures_util::{
    Stream,
    StreamExt,
};

use crate::{
    HttpByteStream,
    HttpError,
    HttpResult,
};

/// Pin-boxed stream of newline-delimited text lines or errors.
pub type SseLineStream = Pin<Box<dyn Stream<Item = HttpResult<String>> + Send>>;

/// Buffers chunks from `stream`, splits on `\n`, strips `\r`, validates UTF-8 per line.
///
/// # Parameters
/// - `stream`: Raw byte stream from [`crate::HttpResponse::stream`].
/// - `max_line_bytes`: Maximum allowed bytes for one SSE line.
///
/// # Returns
/// Stream of lines or [`HttpError::sse_protocol`] on invalid UTF-8.
pub fn decode_lines(mut stream: HttpByteStream, max_line_bytes: usize) -> SseLineStream {
    let output = stream! {
        let max_line_bytes = max_line_bytes.max(1);
        let mut buffer = BytesMut::new();

        while let Some(item) = stream.next().await {
            match item {
                Ok(chunk) => {
                    buffer.extend_from_slice(&chunk);
                    while let Some(index) = buffer.iter().position(|&byte| byte == b'\n') {
                        if index > max_line_bytes {
                            yield Err(HttpError::sse_protocol(format!(
                                "SSE line exceeds max_line_bytes ({max_line_bytes})"
                            )));
                            return;
                        }
                        let mut line = buffer.split_to(index + 1).to_vec();
                        if line.last() == Some(&b'\n') {
                            line.pop();
                        }
                        if line.last() == Some(&b'\r') {
                            line.pop();
                        }

                        match String::from_utf8(line) {
                            Ok(text) => yield Ok(text),
                            Err(error) => {
                                yield Err(HttpError::sse_protocol(format!(
                                    "Failed to decode SSE line as UTF-8: {}",
                                    error
                                )));
                                return;
                            }
                        }
                    }
                    if buffer.len() > max_line_bytes {
                        yield Err(HttpError::sse_protocol(format!(
                            "SSE line exceeds max_line_bytes ({max_line_bytes})"
                        )));
                        return;
                    }
                }
                Err(error) => {
                    yield Err(error);
                    return;
                }
            }
        }

        if !buffer.is_empty() {
            match String::from_utf8(buffer.to_vec()) {
                Ok(text) => yield Ok(text),
                Err(error) => {
                    yield Err(HttpError::sse_protocol(format!(
                        "Failed to decode trailing SSE line as UTF-8: {}",
                        error
                    )));
                }
            }
        }
    };

    Box::pin(output)
}
