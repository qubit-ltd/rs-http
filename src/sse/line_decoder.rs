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

/// Removes the UTF-8 BOM from the first line when present, checks the remaining
/// bytes against the line limit, decodes them as UTF-8, and clears the buffer.
///
/// # Parameters
/// - `buffer`: Current line bytes without the line terminator.
/// - `first_line`: Whether this is the first line; it is set to `false` on
///   every call. A BOM is removed only from this line.
/// - `max_line_bytes`: Maximum number of bytes allowed after removing the
///   first-line BOM, if present.
///
/// # Returns
/// The decoded UTF-8 line text.
///
/// # Errors
/// Returns an SSE protocol error if the bytes exceed `max_line_bytes` after
/// first-line BOM removal or are not valid UTF-8. The buffer is cleared on
/// success and on either error.
fn take_buffered_line(buffer: &mut BytesMut, first_line: &mut bool, max_line_bytes: usize) -> HttpResult<String> {
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
/// BOM at the start of the stream, and validates lines as UTF-8. A final
/// unterminated text fragment is yielded at EOF; SSE framing decides whether
/// it completes an event.
///
/// # Parameters
/// - `stream`: Raw byte stream from [`crate::HttpResponse::stream`].
/// - `max_line_bytes`: Maximum allowed bytes for one SSE line.
///
/// # Returns
/// A stream yielding decoded lines, a final unterminated text fragment, or
/// errors. Input stream errors are forwarded; invalid UTF-8 and lines larger
/// than `max_line_bytes` yield SSE protocol errors and terminate the output
/// stream.
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

        if !buffer.is_empty() {
            match take_buffered_line(&mut buffer, &mut first_line, max_line_bytes) {
                Ok(text) => yield Ok(text),
                Err(error) => yield Err(error),
            }
        }
    };

    Box::pin(output)
}

/// Computes the temporary byte allowance while collecting an SSE line.
///
/// # Parameters
/// - `max_line_bytes`: Maximum logical line length after BOM removal.
/// - `first_line`: Whether the line may start with the stream's UTF-8 BOM.
///
/// # Returns
/// The configured limit, with three extra bytes for the first line only.
/// Addition saturates at usize::MAX. The completed first line is still checked
/// against max_line_bytes after removing its optional leading BOM.
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
    use std::ops::Range;

    use bytes::Bytes;
    use futures_util::StreamExt;

    use super::decode_lines;
    use crate::HttpByteStream;

    type LineCase<'a> = (&'a str, &'a [u8], usize, &'a [&'a str], Option<crate::HttpErrorKind>);

    /// Builds deterministic contiguous chunk layouts for a byte sequence.
    /// Short inputs enumerate every partition; long inputs include whole-input,
    /// bytewise, every single split, and 64 reproducible wrapping-arithmetic
    /// layouts.
    fn chunk_layouts(input: &[u8]) -> Vec<Vec<Range<usize>>> {
        let len = input.len();
        let mut layouts = Vec::new();
        if len <= 12 {
            let partitions = if len <= 1 { 1 } else { 1usize << (len - 1) };
            for mask in 0..partitions {
                let mut cuts = vec![0];
                for offset in 1..len {
                    if mask & (1 << (offset - 1)) != 0 {
                        cuts.push(offset);
                    }
                }
                cuts.push(len);
                layouts.push(cuts.windows(2).map(|pair| pair[0]..pair[1]).collect());
            }
            return layouts;
        }

        layouts.push(std::iter::once(0..len).collect());
        layouts.push((0..len).map(|offset| offset..offset + 1).collect());
        for split in 0..=len {
            layouts.push(vec![0..split, split..len]);
        }
        let fixed_layout_count = layouts.len();
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        while layouts.len() < fixed_layout_count + 64 {
            let mut cuts = vec![0];
            for offset in 1..len {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                if state.is_multiple_of(5) {
                    cuts.push(offset);
                }
            }
            cuts.push(len);
            layouts.push(cuts.windows(2).map(|pair| pair[0]..pair[1]).collect());
        }
        layouts
    }

    fn stream_for_layout(input: &[u8], layout: &[Range<usize>]) -> HttpByteStream {
        let mut chunks = Vec::with_capacity(layout.len() * 2 + 2);
        chunks.push(Ok(Bytes::new()));
        for (index, range) in layout.iter().enumerate() {
            chunks.push(Ok(Bytes::copy_from_slice(&input[range.clone()])));
            if index % 2 == 0 {
                chunks.push(Ok(Bytes::new()));
            }
        }
        chunks.push(Ok(Bytes::new()));
        Box::pin(futures_util::stream::iter(chunks))
    }

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
    async fn test_decode_lines_yields_unterminated_eof_fragment() {
        assert_eq!(decode(vec![b"data: value"], 64).await.unwrap(), vec!["data: value"]);
        assert_eq!(decode(vec![b"data: value\n"], 64).await.unwrap(), vec!["data: value"]);
        assert_eq!(
            decode(vec![b"data: value\n\n"], 64).await.unwrap(),
            vec!["data: value", ""]
        );
        for ending in [b"\r".as_slice(), b"\n", b"\r\n"] {
            assert_eq!(decode(vec![b"line", ending], 64).await.unwrap(), vec!["line"]);
        }
    }

    #[tokio::test]
    async fn test_decode_lines_ignores_one_bom_only_at_stream_start() {
        assert_eq!(
            decode(vec![b"", b"data: empty chunk ok\n"], 64).await.unwrap(),
            vec!["data: empty chunk ok"]
        );
        assert_eq!(
            decode(vec![b"\xef\xbb\xbfdata: first\r\n\r\n"], 64).await.unwrap(),
            vec!["data: first", ""]
        );
        assert_eq!(decode(vec![b"data"], 64).await.unwrap(), vec!["data"]);
        assert_eq!(decode(vec![b"\xef\xbb\xbf\n"], 64).await.unwrap(), vec![""]);
        assert_eq!(decode(vec![b"\xef\xbb\xbf"], 64).await.unwrap(), vec![""]);
        assert_eq!(
            decode(vec![b"\xef\xbb\xbf\xef\xbb\xbfX\n"], 64).await.unwrap(),
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
        let single: HttpByteStream = Box::pin(futures_util::stream::iter(vec![Ok(Bytes::copy_from_slice(source))]));
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
        assert_eq!(decode(vec![b"\xef\xbb\xbfabcd\n"], 4).await.unwrap(), vec!["abcd"]);
        assert_eq!(
            decode(vec![b"\xef\xbb\xbfabcde\n"], 4).await.unwrap_err().kind,
            crate::HttpErrorKind::SseProtocol
        );
        assert_eq!(
            decode(vec![b"\n\xef\xbb\xbfabcd\n"], 4).await.unwrap_err().kind,
            crate::HttpErrorKind::SseProtocol
        );
        assert_eq!(
            decode(vec![b"\xff\n"], 4).await.unwrap_err().kind,
            crate::HttpErrorKind::SseProtocol
        );
        assert_eq!(
            decode(vec![b"\xff"], 4).await.unwrap_err().kind,
            crate::HttpErrorKind::SseProtocol
        );
    }

    #[tokio::test]
    async fn test_decode_lines_multichunk_corpus_is_partition_invariant() {
        let cases: &[LineCase<'_>] = &[
            ("short-all-cuts", b"x\r\ny\n", 8, &["x", "y"], None),
            (
                "bom-mixed-newlines-utf8",
                "\u{feff}café\r\n\rnext\nlast".as_bytes(),
                16,
                &["café", "", "next", "last"],
                None,
            ),
            ("line-at-limit", b"abcd\n", 4, &["abcd"], None),
            (
                "line-over-limit",
                b"ok\nabcdE\n",
                4,
                &["ok"],
                Some(crate::HttpErrorKind::SseProtocol),
            ),
            (
                "invalid-utf8",
                b"valid\n\xff\n",
                16,
                &["valid"],
                Some(crate::HttpErrorKind::SseProtocol),
            ),
            (
                "eof-fragment",
                b"tail without newline",
                32,
                &["tail without newline"],
                None,
            ),
        ];

        for (name, source, limit, expected_lines, expected_error) in cases {
            for (layout_index, layout) in chunk_layouts(source).iter().enumerate() {
                let actual = decode_lines(stream_for_layout(source, layout), *limit)
                    .collect::<Vec<_>>()
                    .await;
                let context = format!("corpus={name}, limit={limit}, layout={layout_index}, chunks={layout:?}");
                assert_eq!(
                    actual.len(),
                    expected_lines.len() + usize::from(expected_error.is_some()),
                    "{context}"
                );
                for (item, expected) in actual.iter().zip(*expected_lines) {
                    assert_eq!(item.as_ref().expect("expected successful line"), expected, "{context}");
                }
                match expected_error {
                    Some(kind) => {
                        assert_eq!(
                            actual
                                .get(expected_lines.len())
                                .and_then(|item| item.as_ref().err())
                                .map(|error| error.kind),
                            Some(*kind),
                            "{context}"
                        );
                        assert_eq!(
                            actual.len(),
                            expected_lines.len() + 1,
                            "the first error must terminate the stream: {context}"
                        );
                    }
                    None => assert_eq!(actual.len(), expected_lines.len(), "{context}"),
                }
            }
        }
    }
}
