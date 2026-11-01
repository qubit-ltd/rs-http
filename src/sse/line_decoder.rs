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
use bytes::Buf;
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
fn take_buffered_line(
    buffer: &mut BytesMut,
    first_line: &mut bool,
    max_line_bytes: usize,
) -> HttpResult<String> {
    if *first_line {
        *first_line = false;
        if buffer.starts_with(b"\xef\xbb\xbf") {
            buffer.advance(3);
        }
    }
    if buffer.len() > max_line_bytes {
        buffer.clear();
        return Err(HttpError::sse_protocol(format!(
            "SSE line exceeds max_line_bytes ({})",
            max_line_bytes,
        )));
    }
    let decoded = match std::str::from_utf8(buffer.as_ref()) {
        Ok(text) => Ok(text.to_string()),
        Err(error) => Err(HttpError::sse_protocol(format!(
            "Failed to decode SSE line as UTF-8: {error}"
        ))),
    };
    buffer.clear();
    decoded
}

/// Buffers chunks from `stream`, splits on LF, CR, or CRLF, ignores one UTF-8
/// BOM at the start of the stream, and validates complete lines as UTF-8. An
/// unterminated final line is discarded at EOF.
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
        let mut first_line = true;

        while let Some(item) = stream.next().await {
            match item {
                Ok(chunk) => {
                    let mut remaining = chunk.as_ref();
                    while !remaining.is_empty() {
                        let Some(index) = remaining.iter().position(|byte| is_line_terminator(*byte)) else {
                            pending_cr = false;
                            if let Err(error) =
                                append_line_bytes(&mut buffer, remaining, line_limit(max_line_bytes, first_line))
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
                        if let Err(error) = append_line_bytes(&mut buffer, segment, line_limit(max_line_bytes, first_line)) {
                            yield Err(error);
                            return;
                        }

                        let terminator = remaining[index];
                        remaining = &remaining[index + 1..];
                        if terminator == b'\n' && pending_cr {
                            pending_cr = false;
                            continue;
                        }

                        match take_buffered_line(&mut buffer, &mut first_line, max_line_bytes) {
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

        // An unterminated final line is not an SSE line and must be discarded.
    };

    Box::pin(output)
}

fn line_limit(max_line_bytes: usize, first_line: bool) -> usize {
    if first_line {
        max_line_bytes.saturating_add(3)
    } else {
        max_line_bytes
    }
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

#[cfg(test)]
mod tests {
    use super::decode_lines;
    use crate::HttpByteStream;
    use bytes::Bytes;
    use futures_util::StreamExt;

    async fn decode(chunks: Vec<&'static [u8]>, limit: usize) -> crate::HttpResult<Vec<String>> {
        let chunks = chunks.into_iter().map(Bytes::from_static).map(Ok);
        let input: HttpByteStream = Box::pin(futures_util::stream::iter(chunks));
        decode_lines(input, limit)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect()
    }

    #[tokio::test]
    async fn test_decode_lines_discards_unterminated_eof_and_accepts_complete_lines() {
        assert_eq!(
            decode(vec![b"data: value"], 64).await.unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            decode(vec![b"data: value\n"], 64).await.unwrap(),
            vec!["data: value"]
        );
        assert_eq!(
            decode(vec![b"data: value\n\n"], 64).await.unwrap(),
            vec!["data: value", ""]
        );
        for ending in [b"\r".as_slice(), b"\n", b"\r\n"] {
            assert_eq!(
                decode(vec![b"line", ending], 64).await.unwrap(),
                vec!["line"]
            );
        }
    }

    #[tokio::test]
    async fn test_decode_lines_ignores_one_bom_only_at_stream_start() {
        assert_eq!(
            decode(vec![b"", b"data: empty chunk ok\n"], 64)
                .await
                .unwrap(),
            vec!["data: empty chunk ok"]
        );
        assert_eq!(
            decode(vec![b"\xef\xbb\xbfdata: first\r\n\r\n"], 64)
                .await
                .unwrap(),
            vec!["data: first", ""]
        );
        assert_eq!(
            decode(vec![b"data"], 64).await.unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(decode(vec![b"\xef\xbb\xbf\n"], 64).await.unwrap(), vec![""]);
        assert_eq!(
            decode(vec![b"\xef\xbb\xbf"], 64).await.unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            decode(vec![b"\xef\xbb\xbf\xef\xbb\xbfX\n"], 64)
                .await
                .unwrap(),
            vec!["\u{feff}X"]
        );
        assert_eq!(
            decode(vec![b"\n\xef\xbb\xbfX\n"], 64).await.unwrap(),
            vec!["", "\u{feff}X"]
        );
        assert_eq!(
            decode(vec![b"data: \xef\xbb\xbfX\n"], 64).await.unwrap(),
            vec!["data: \u{feff}X"]
        );
    }

    #[tokio::test]
    async fn test_decode_lines_real_chunk_boundaries_match_single_chunk() {
        let source = "\u{feff}data: café\r\n\r\nevent: done\ndata: second\n\n".as_bytes();
        let expected = vec!["data: café", "", "event: done", "data: second", ""];
        let single: HttpByteStream = Box::pin(futures_util::stream::iter(vec![Ok(
            Bytes::copy_from_slice(source),
        )]));
        let actual = decode_lines(single, 64)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<crate::HttpResult<Vec<_>>>()
            .unwrap();
        assert_eq!(actual, expected);
        let byte_chunks = source
            .iter()
            .map(|byte| Ok(Bytes::copy_from_slice(&[*byte])))
            .collect::<Vec<_>>();
        let input: HttpByteStream = Box::pin(futures_util::stream::iter(byte_chunks));
        let actual = decode_lines(input, 64)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<crate::HttpResult<Vec<_>>>()
            .unwrap();
        assert_eq!(actual, expected);
        for split in 0..=source.len() {
            let chunks = vec![
                Ok(Bytes::copy_from_slice(&source[..split])),
                Ok(Bytes::copy_from_slice(&source[split..])),
            ];
            let input: HttpByteStream = Box::pin(futures_util::stream::iter(chunks));
            let actual = decode_lines(input, 64)
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .collect::<crate::HttpResult<Vec<_>>>()
                .unwrap();
            assert_eq!(actual, expected, "split at byte {split}");
        }
        let chunks = vec![
            Ok(Bytes::from_static(b"\xef")),
            Ok(Bytes::from_static(b"\xbb")),
            Ok(Bytes::from_static(b"\xbfdata: first\r")),
            Ok(Bytes::from_static(b"\n\r")),
            Ok(Bytes::from_static(b"\n")),
        ];
        let input: HttpByteStream = Box::pin(futures_util::stream::iter(chunks));
        let actual = decode_lines(input, 64)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<crate::HttpResult<Vec<_>>>()
            .unwrap();
        assert_eq!(actual, vec!["data: first", ""]);
    }

    #[tokio::test]
    async fn test_decode_lines_enforces_logical_line_byte_limit() {
        assert_eq!(decode(vec![b"abcd\n"], 4).await.unwrap(), vec!["abcd"]);
        assert_eq!(
            decode(vec![b"abcde\n"], 4).await.unwrap_err().kind,
            crate::HttpErrorKind::SseProtocol
        );
        assert_eq!(
            decode(vec![b"\xef\xbb\xbfabcd\n"], 4).await.unwrap(),
            vec!["abcd"]
        );
        assert_eq!(
            decode(vec![b"\xef\xbb\xbfabcde\n"], 4)
                .await
                .unwrap_err()
                .kind,
            crate::HttpErrorKind::SseProtocol
        );
        assert_eq!(
            decode(vec![b"\n\xef\xbb\xbfabcd\n"], 4)
                .await
                .unwrap_err()
                .kind,
            crate::HttpErrorKind::SseProtocol
        );
        assert_eq!(
            decode(vec![b"\xff\n"], 4).await.unwrap_err().kind,
            crate::HttpErrorKind::SseProtocol
        );
    }
}
