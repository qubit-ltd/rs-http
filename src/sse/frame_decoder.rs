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
/// - `max_frame_bytes`: Maximum cumulative UTF-8 bytes of non-empty,
///   non-comment lines between blank-line boundaries. Field names and values,
///   including unknown fields, count; line terminators and comment lines do
///   not. Comment lines remain subject to the line limit. This is not a
///   wire-size or whole-stream limit.
///
/// # Returns
/// Stream of records or forwarded transport/protocol errors.
pub(crate) fn decode_records(mut lines: SseLineStream, max_frame_bytes: usize) -> SseRecordStream {
    let output = stream! {
        let max_frame_bytes = max_frame_bytes.max(1);
        let mut current_event: Option<String> = None;
        let mut current_id: Option<String> = None;
        let mut current_data = String::new();
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
                    if let Some(id) = current_id.take() {
                        last_event_id = Some(id.clone());
                        if !has_data {
                            yield Ok(SseRecord::Control(SseControl::LastEventId(id)));
                        }
                    }
                    if has_data {
                        yield Ok(SseRecord::Dispatch(SseMessage {
                            event: current_event.take(),
                            data: std::mem::take(&mut current_data),
                            last_event_id: last_event_id.clone(),
                        }));
                    } else {
                        current_event.take();
                    }
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
                    if has_data {
                        current_data.push('\n');
                    }
                    current_data.push_str(value);
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
                    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
                        if let Ok(retry) = value.parse::<u64>() {
                            yield Ok(SseRecord::Control(SseControl::ReconnectDelayMs(retry)));
                        }
                    }
                    has_fields = true;
                }
                _ => {}
            }
        }

        // Preserve complete id control fields at EOF, but never dispatch an
        // event whose terminating blank line was missing.
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
    use std::ops::Range;

    use bytes::Bytes;
    use futures_util::StreamExt;

    use super::decode_records;
    use crate::HttpByteStream;
    use crate::sse::SseControl;
    use crate::sse::SseRecord;
    use crate::sse::line_decoder::decode_lines;

    type FrameCase<'a> = (&'a str, &'a [u8], usize, &'a [SseRecord], Option<crate::HttpErrorKind>);

    fn chunk_layouts(input: &[u8]) -> Vec<Vec<Range<usize>>> {
        let len = input.len();
        if len <= 12 {
            let partitions = if len <= 1 { 1 } else { 1usize << (len - 1) };
            return (0..partitions)
                .map(|mask| {
                    let mut cuts = vec![0];
                    for offset in 1..len {
                        if mask & (1 << (offset - 1)) != 0 {
                            cuts.push(offset);
                        }
                    }
                    cuts.push(len);
                    cuts.windows(2).map(|pair| pair[0]..pair[1]).collect()
                })
                .collect();
        }

        let mut layouts = vec![vec![0..len], (0..len).map(|offset| offset..offset + 1).collect()];
        for split in 0..=len {
            layouts.push(vec![0..split, split..len]);
        }
        let fixed_layout_count = layouts.len();
        let mut state = 0xd1b5_4a32_d192_ed03u64;
        while layouts.len() < fixed_layout_count + 64 {
            let mut cuts = vec![0];
            for offset in 1..len {
                state = state
                    .wrapping_mul(2_862_933_555_777_941_757)
                    .wrapping_add(3_037_000_493);
                if state % 7 < 2 {
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

    async fn decode_chunks(chunks: Vec<&'static [u8]>) -> crate::HttpResult<Vec<SseRecord>> {
        let input: HttpByteStream = Box::pin(futures_util::stream::iter(
            chunks.into_iter().map(Bytes::from_static).map(Ok),
        ));
        let lines = decode_lines(input, 64);
        decode_records(lines, 1024)
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
    async fn test_decode_records_accepts_only_ascii_digit_retry_values() {
        for (line, retry) in [
            (b"retry: +120\n\n" as &'static [u8], "+120"),
            (b"retry: -1\n\n", "-1"),
            (b"retry:\n\n", ""),
            (b"retry:  120\n\n", " 120"),
            (b"retry: \xD9\xA1\xD9\xA2\xD9\xA0\n\n", "١٢٠"),
            (b"retry: 18446744073709551616\n\n", "18446744073709551616"),
        ] {
            let records = decode(vec![line])
                .await
                .expect("invalid retry values are ignored without protocol errors");
            assert!(records.is_empty(), "retry value {retry:?} must be ignored");
        }

        for (line, expected) in [(b"retry: 0\n\n" as &'static [u8], 0), (b"retry: 120\n\n", 120)] {
            let records = decode(vec![line]).await.expect("valid retry value should decode");
            assert_eq!(
                records,
                vec![SseRecord::Control(SseControl::ReconnectDelayMs(expected))]
            );
        }

        let records = decode(vec![b"retry: 120\nretry: +120\n\n"])
            .await
            .expect("an invalid retry must not fail decoding");
        assert_eq!(records, vec![SseRecord::Control(SseControl::ReconnectDelayMs(120))]);
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

    #[tokio::test]
    async fn test_frame_budget_counts_field_bytes_but_not_comments_or_terminators() {
        for source in [
            b": heartbeat\ndata: a\n\n".as_slice(),
            b": heartbeat\r\ndata: a\r\n\r\n".as_slice(),
        ] {
            let records = decode_with_limit(vec![source], 7).await.unwrap();
            assert_eq!(records.len(), 1);
            let SseRecord::Dispatch(message) = &records[0] else {
                panic!("expected one data dispatch");
            };
            assert_eq!(message.data, "a");
            let error = decode_with_limit(vec![source], 6).await.unwrap_err();
            assert_eq!(error.kind(), crate::HttpErrorKind::SseProtocol);
        }
    }

    #[tokio::test]
    async fn test_frame_budget_counts_unknown_fields_and_utf8_bytes() {
        let source = b"x: 1\ndata: a\n\n";
        assert_eq!(decode_with_limit(vec![source], 11).await.unwrap().len(), 1);
        assert_eq!(
            decode_with_limit(vec![source], 10).await.unwrap_err().kind(),
            crate::HttpErrorKind::SseProtocol,
        );
        let source = "data: é\n\n".as_bytes();
        assert_eq!(decode_with_limit(vec![source], 8).await.unwrap().len(), 1);
        assert_eq!(
            decode_with_limit(vec![source], 7).await.unwrap_err().kind(),
            crate::HttpErrorKind::SseProtocol,
        );
    }

    #[tokio::test]
    async fn test_frame_decoder_handles_fields_split_across_real_byte_chunks() {
        let records = decode_chunks(vec![b"data: alpha\ndata:", b"\ndata: omega\n", b"\n"])
            .await
            .expect("fields split between byte chunks should decode");

        assert_eq!(
            records,
            vec![SseRecord::Dispatch(crate::sse::SseMessage {
                event: None,
                data: "alpha\n\nomega".to_owned(),
                last_event_id: None,
            })]
        );
    }

    #[tokio::test]
    async fn test_frame_decoder_multichunk_corpus_is_partition_invariant() {
        let source = "\u{feff}: keepalive\r\nid: seed\r\ndata: café\r\ndata: second\r\n\r\ndata: inherited\n\n\nevent: named\ndata: x\nid:\n\n\ndata: reset-inherited\n\nretry: 9\nx-extra: ignored\n\n"
            .as_bytes();
        let expected = vec![
            SseRecord::Dispatch(crate::sse::SseMessage {
                event: None,
                data: "café\nsecond".to_owned(),
                last_event_id: Some("seed".to_owned()),
            }),
            SseRecord::Dispatch(crate::sse::SseMessage {
                event: None,
                data: "inherited".to_owned(),
                last_event_id: Some("seed".to_owned()),
            }),
            SseRecord::Dispatch(crate::sse::SseMessage {
                event: Some("named".to_owned()),
                data: "x".to_owned(),
                last_event_id: Some(String::new()),
            }),
            SseRecord::Dispatch(crate::sse::SseMessage {
                event: None,
                data: "reset-inherited".to_owned(),
                last_event_id: Some(String::new()),
            }),
            SseRecord::Control(SseControl::ReconnectDelayMs(9)),
        ];
        for (layout_index, layout) in chunk_layouts(source).iter().enumerate() {
            let lines = decode_lines(stream_for_layout(source, layout), 64);
            let actual = decode_records(lines, 1024).collect::<Vec<_>>().await;
            assert_eq!(
                actual.len(),
                expected.len(),
                "corpus=fields, limit=1024, layout={layout_index}, chunks={layout:?}"
            );
            for (record, expected_record) in actual.iter().zip(&expected) {
                assert_eq!(
                    record.as_ref().expect("valid corpus"),
                    expected_record,
                    "layout={layout_index}, chunks={layout:?}"
                );
            }
        }
    }

    #[tokio::test]
    async fn test_frame_decoder_multichunk_limits_eof_and_invalid_utf8() {
        let cases: &[FrameCase<'_>] = &[
            (
                "frame-at-limit",
                b"data: a\n\n",
                7,
                &[SseRecord::Dispatch(crate::sse::SseMessage {
                    event: None,
                    data: "a".to_owned(),
                    last_event_id: None,
                })],
                None,
            ),
            (
                "frame-over-limit",
                b"data: a\n\n",
                6,
                &[],
                Some(crate::HttpErrorKind::SseProtocol),
            ),
            ("unknown-field-counts", b"x: 1\n\n", 4, &[], None),
            (
                "unknown-field-over-limit",
                b"x: 1\n\n",
                3,
                &[],
                Some(crate::HttpErrorKind::SseProtocol),
            ),
            ("eof-incomplete-event", b"data: pending", 64, &[], None),
            (
                "eof-id-control",
                b"id: eof",
                64,
                &[SseRecord::Control(SseControl::LastEventId("eof".to_owned()))],
                None,
            ),
            (
                "invalid-utf8-after-event",
                b"data: ok\n\n\xff\n",
                64,
                &[SseRecord::Dispatch(crate::sse::SseMessage {
                    event: None,
                    data: "ok".to_owned(),
                    last_event_id: None,
                })],
                Some(crate::HttpErrorKind::SseProtocol),
            ),
        ];

        for (name, source, limit, expected_prefix, expected_error) in cases {
            for (layout_index, layout) in chunk_layouts(source).iter().enumerate() {
                let lines = decode_lines(stream_for_layout(source, layout), 64);
                let actual = decode_records(lines, *limit).collect::<Vec<_>>().await;
                let context = format!("corpus={name}, limit={limit}, layout={layout_index}, chunks={layout:?}");
                assert_eq!(
                    actual.len(),
                    expected_prefix.len() + usize::from(expected_error.is_some()),
                    "{context}"
                );
                for (item, expected) in actual.iter().zip(*expected_prefix) {
                    assert_eq!(
                        item.as_ref().expect("expected successful record"),
                        expected,
                        "{context}"
                    );
                }
                match expected_error {
                    Some(kind) => {
                        assert_eq!(
                            actual
                                .get(expected_prefix.len())
                                .and_then(|item| item.as_ref().err())
                                .map(crate::HttpError::kind),
                            Some(*kind),
                            "{context}"
                        );
                        assert_eq!(
                            actual.len(),
                            expected_prefix.len() + 1,
                            "the first error must terminate the stream: {context}"
                        );
                    }
                    None => assert_eq!(actual.len(), expected_prefix.len(), "{context}"),
                }
            }
        }
    }

    #[tokio::test]
    async fn test_frame_decoder_transport_error_uses_fixed_logical_offset() {
        let source = b"data: first\n\n";
        for (layout_index, layout) in chunk_layouts(source).iter().enumerate() {
            let mut chunks = vec![Ok(Bytes::new())];
            for (index, range) in layout.iter().enumerate() {
                chunks.push(Ok(Bytes::copy_from_slice(&source[range.clone()])));
                if index % 2 == 0 {
                    chunks.push(Ok(Bytes::new()));
                }
            }
            chunks.push(Err(crate::HttpError::new(
                crate::HttpErrorKind::Transport,
                "injected transport error",
            )));
            let input: HttpByteStream = Box::pin(futures_util::stream::iter(chunks));
            let actual = decode_records(decode_lines(input, 64), 1024).collect::<Vec<_>>().await;
            assert_eq!(
                actual.len(),
                2,
                "transport error at logical offset {}, layout={layout_index}",
                source.len()
            );
            assert_eq!(
                actual[0].as_ref().unwrap(),
                &SseRecord::Dispatch(crate::sse::SseMessage {
                    event: None,
                    data: "first".to_owned(),
                    last_event_id: None,
                })
            );
            assert_eq!(actual[1].as_ref().unwrap_err().kind(), crate::HttpErrorKind::Transport);
        }
    }
}
