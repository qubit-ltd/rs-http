// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Generated SSE invariants with bounded inputs and deterministic seeds.

use futures_util::StreamExt;
use proptest::prelude::Just;
use proptest::prelude::Strategy;
use proptest::prelude::any;
use proptest::prop_assert_eq;
use proptest::test_runner::Config as ProptestConfig;
use proptest::test_runner::RngSeed;
use proptest::test_runner::TestRunner;

use super::generated_cases::GeneratedCase;
use super::generated_cases::case_strategy;
use super::generated_cases::stream_for_case;
use super::generated_cases::stream_with_error;
use crate::HttpErrorKind;
use crate::sse::SseControl;
use crate::sse::SseMessage;
use crate::sse::SseRecord;
use crate::sse::decode_records_from_stream_with_limits;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Outcome {
    Record(SseRecord),
    Error(HttpErrorKind),
}

fn decode(runtime: &tokio::runtime::Runtime, case: &GeneratedCase, cuts: &[usize]) -> Vec<Outcome> {
    decode_stream(runtime, stream_for_case(case, cuts), case.line_limit, case.frame_limit)
}

fn decode_stream(
    runtime: &tokio::runtime::Runtime,
    stream: crate::HttpByteStream,
    line_limit: usize,
    frame_limit: usize,
) -> Vec<Outcome> {
    runtime.block_on(async {
        let mut stream = decode_records_from_stream_with_limits(stream, line_limit, frame_limit);
        let mut output = Vec::new();
        while let Some(item) = stream.next().await {
            match item {
                Ok(record) => output.push(Outcome::Record(record)),
                Err(error) => {
                    output.push(Outcome::Error(error.kind));
                    assert!(stream.next().await.is_none(), "decoder emitted an item after an error");
                    break;
                }
            }
        }
        output
    })
}

fn runner(seed: u64) -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases: 128,
        max_shrink_iters: 2048,
        rng_seed: RngSeed::Fixed(seed),
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
}

#[test]
fn test_generated_chunk_layouts_preserve_records_and_error_prefix() {
    let mut runner = runner(20261108);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runner
        .run(&case_strategy(), |case| {
            let bytewise: Vec<usize> = (1..case.bytes.len()).collect();
            let contiguous = decode(&runtime, &case, &[]);
            prop_assert_eq!(decode(&runtime, &case, &bytewise), contiguous.clone());
            prop_assert_eq!(decode(&runtime, &case, &case.cuts), contiguous);
            Ok(())
        })
        .expect("generated line/frame chunk invariant");
}

#[test]
fn test_generated_structured_messages_match_independent_expected_records() {
    let strategy = (
        "[a-z]{0,12}",
        "[a-z]{0,12}",
        "[a-z]{0,12}",
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(event, first, second, crlf, terminated)| {
            let ending = if crlf { "\r\n" } else { "\n" };
            let suffix = if terminated {
                format!("{ending}{ending}")
            } else {
                String::new()
            };
            let bytes = format!("event: {event}{ending}data: {first}{ending}data: {second}{suffix}").into_bytes();
            (bytes, event, first, second, terminated)
        });
    let mut runner = runner(20261109);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runner
        .run(&strategy, |(bytes, event, first, second, terminated)| {
            let case = GeneratedCase {
                bytes,
                line_limit: 128,
                frame_limit: 512,
                cuts: Vec::new(),
                error_offset: None,
            };
            let expected = if terminated {
                vec![Outcome::Record(SseRecord::Dispatch(SseMessage {
                    event: Some(event),
                    data: format!("{first}\n{second}"),
                    last_event_id: None,
                }))]
            } else {
                Vec::new()
            };
            prop_assert_eq!(decode(&runtime, &case, &[]), expected);
            Ok(())
        })
        .expect("generated structured semantic model");
}

#[test]
fn test_generated_field_semantics_match_independent_expected_records() {
    let ending = proptest::prop_oneof![Just("\n"), Just("\r"), Just("\r\n")];
    let strategy = (
        "[a-z]{0,8}",
        "[a-z]{0,8}",
        "[a-z]{0,8}",
        "[a-z]{0,8}",
        any::<bool>(),
        0u64..=u64::MAX,
        0u8..=3,
        0u8..=4,
        any::<bool>(),
        ending,
    )
        .prop_map(
            |(prior_id, event, first, second, has_prior_id, valid_retry, id_mode, retry_mode, bom, ending)| {
                let mut input = String::new();
                if bom {
                    input.push('\u{feff}');
                }
                input.push_str(": generated comment");
                input.push_str(ending);
                input.push_str("x-generated: ignored");
                input.push_str(ending);
                match retry_mode {
                    0 => {}
                    1 => {
                        input.push_str(&format!("retry: {valid_retry}"));
                        input.push_str(ending);
                    }
                    2 => {
                        input.push_str("retry: +1");
                        input.push_str(ending);
                    }
                    3 => {
                        input.push_str("retry:");
                        input.push_str(ending);
                    }
                    _ => {
                        input.push_str("retry: 18446744073709551616");
                        input.push_str(ending);
                    }
                }
                input.push_str(ending);
                if has_prior_id {
                    input.push_str("id: ");
                    input.push_str(&prior_id);
                    input.push_str(ending);
                    input.push_str(ending);
                }
                input.push_str("event: ");
                input.push_str(&event);
                input.push_str(ending);
                let current_id = match id_mode {
                    1 => {
                        input.push_str("id: generated");
                        input.push_str(ending);
                        Some("generated".to_owned())
                    }
                    2 => {
                        input.push_str("id:");
                        input.push_str(ending);
                        Some(String::new())
                    }
                    3 => {
                        input.push_str("id: invalid\0cursor");
                        input.push_str(ending);
                        None
                    }
                    _ => None,
                };
                input.push_str("data: ");
                input.push_str(&first);
                input.push_str(ending);
                input.push_str("data: ");
                input.push_str(&second);
                input.push_str(ending);
                input.push_str(ending);

                let mut expected = Vec::new();
                if retry_mode == 1 {
                    expected.push(Outcome::Record(SseRecord::Control(SseControl::ReconnectDelayMs(
                        valid_retry,
                    ))));
                }
                if has_prior_id {
                    expected.push(Outcome::Record(SseRecord::Control(SseControl::LastEventId(
                        prior_id.clone(),
                    ))));
                }
                let last_event_id = current_id.or_else(|| has_prior_id.then_some(prior_id));
                expected.push(Outcome::Record(SseRecord::Dispatch(SseMessage {
                    event: Some(event),
                    data: format!("{first}\n{second}"),
                    last_event_id,
                })));
                (input.into_bytes(), expected)
            },
        );
    let mut runner = runner(20261110);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runner
        .run(&strategy, |(bytes, expected)| {
            let case = GeneratedCase {
                bytes,
                line_limit: 512,
                frame_limit: 1024,
                cuts: Vec::new(),
                error_offset: None,
            };
            prop_assert_eq!(decode(&runtime, &case, &[]), expected);
            Ok(())
        })
        .expect("generated independent field semantics");
}

#[test]
fn test_generated_line_limit_is_inclusive_and_rejects_one_more_byte() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let allowed = GeneratedCase {
        bytes: b"data: a\n\n".to_vec(),
        line_limit: 7,
        frame_limit: 32,
        cuts: vec![1],
        error_offset: None,
    };
    assert_eq!(
        decode(&runtime, &allowed, &[]),
        vec![Outcome::Record(SseRecord::Dispatch(SseMessage {
            event: None,
            data: "a".to_owned(),
            last_event_id: None
        }))]
    );
    let with_bom = GeneratedCase {
        bytes: b"\xef\xbb\xbfdata: a\n\n".to_vec(),
        line_limit: 7,
        frame_limit: 32,
        cuts: vec![1, 2, 3],
        error_offset: None,
    };
    assert_eq!(decode(&runtime, &with_bom, &[]), decode(&runtime, &allowed, &[]));
    let unicode_allowed = GeneratedCase {
        bytes: "data: é\n\n".as_bytes().to_vec(),
        line_limit: 8,
        frame_limit: 32,
        cuts: Vec::new(),
        error_offset: None,
    };
    assert_eq!(
        decode(&runtime, &unicode_allowed, &[]),
        vec![Outcome::Record(SseRecord::Dispatch(SseMessage {
            event: None,
            data: "é".to_owned(),
            last_event_id: None,
        }))]
    );
    let over = GeneratedCase {
        bytes: b"data: aa\n\n".to_vec(),
        line_limit: 7,
        frame_limit: 32,
        cuts: vec![7],
        error_offset: None,
    };
    assert_eq!(
        decode(&runtime, &over, &[]),
        vec![Outcome::Error(HttpErrorKind::SseProtocol)]
    );
}

#[test]
fn test_generated_frame_limit_counts_unknown_fields_and_preserves_prefix() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let input = b"data: ok\n\nx: 123456\n\n".to_vec();
    let case = GeneratedCase {
        bytes: input,
        line_limit: 32,
        frame_limit: 8,
        cuts: vec![2, 8],
        error_offset: None,
    };
    assert_eq!(
        decode(&runtime, &case, &[]),
        vec![
            Outcome::Record(SseRecord::Dispatch(SseMessage {
                event: None,
                data: "ok".to_owned(),
                last_event_id: None
            })),
            Outcome::Error(HttpErrorKind::SseProtocol),
        ]
    );
    let control_eof = GeneratedCase {
        bytes: b"retry: 23\nid: eof-cursor".to_vec(),
        line_limit: 32,
        frame_limit: 64,
        cuts: vec![2, 9, 10],
        error_offset: None,
    };
    assert_eq!(
        decode(&runtime, &control_eof, &[]),
        vec![
            Outcome::Record(SseRecord::Control(SseControl::ReconnectDelayMs(23))),
            Outcome::Record(SseRecord::Control(SseControl::LastEventId("eof-cursor".to_owned(),))),
        ]
    );
    let comment_and_terminator = GeneratedCase {
        bytes: b": a comment that exceeds the frame budget\ndata: a\n\n".to_vec(),
        line_limit: 64,
        frame_limit: 7,
        cuts: Vec::new(),
        error_offset: None,
    };
    assert_eq!(
        decode(&runtime, &comment_and_terminator, &[]),
        vec![Outcome::Record(SseRecord::Dispatch(SseMessage {
            event: None,
            data: "a".to_owned(),
            last_event_id: None,
        }))]
    );
    let unicode_frame = GeneratedCase {
        bytes: "data: é\n\n".as_bytes().to_vec(),
        line_limit: 32,
        frame_limit: 8,
        cuts: Vec::new(),
        error_offset: None,
    };
    assert_eq!(
        decode(&runtime, &unicode_frame, &[]),
        vec![Outcome::Record(SseRecord::Dispatch(SseMessage {
            event: None,
            data: "é".to_owned(),
            last_event_id: None,
        }))]
    );
}

#[test]
fn test_generated_transport_error_is_injected_at_a_logical_offset() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let prefix = b"data: first\n\ndata: pending\n";
    for split in 0..=prefix.len() {
        let case = GeneratedCase {
            bytes: prefix.to_vec(),
            line_limit: 128,
            frame_limit: 512,
            cuts: Vec::new(),
            error_offset: Some(split),
        };
        let result = decode_stream(
            &runtime,
            stream_with_error(&case, &[split / 2], split),
            case.line_limit,
            case.frame_limit,
        );
        assert_eq!(
            result
                .iter()
                .filter(|item| **item == Outcome::Error(HttpErrorKind::Transport))
                .count(),
            1
        );
        assert_eq!(result.last(), Some(&Outcome::Error(HttpErrorKind::Transport)));
        let expected_prefix = if split >= b"data: first\n\n".len() {
            vec![Outcome::Record(SseRecord::Dispatch(SseMessage {
                event: None,
                data: "first".to_owned(),
                last_event_id: None,
            }))]
        } else {
            Vec::new()
        };
        assert_eq!(&result[..result.len() - 1], expected_prefix);
    }
}

#[test]
fn test_generated_control_fields_and_eof_contract_have_independent_expectations() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let case = GeneratedCase {
        bytes: b"retry: 17\nid: cursor\n\n".to_vec(),
        line_limit: 32,
        frame_limit: 64,
        cuts: vec![1, 2, 3],
        error_offset: None,
    };
    assert_eq!(
        decode(&runtime, &case, &[]),
        vec![
            Outcome::Record(SseRecord::Control(SseControl::ReconnectDelayMs(17))),
            Outcome::Record(SseRecord::Control(SseControl::LastEventId("cursor".to_owned()))),
        ]
    );
    let eof = GeneratedCase {
        bytes: b"data: not dispatched".to_vec(),
        line_limit: 64,
        frame_limit: 64,
        cuts: vec![4],
        error_offset: None,
    };
    assert_eq!(decode(&runtime, &eof, &[]), Vec::<Outcome>::new());
}

#[test]
fn test_generated_transport_layouts_preserve_success_prefix_and_emit_error_once() {
    let mut runner = runner(20261109);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let strategy = ("[a-z]{0,16}", "[a-z]{0,16}", any::<bool>()).prop_flat_map(|(first, second, include_second)| {
        let first_message = format!("data: {first}\n\n").into_bytes();
        let second_message = format!("data: {second}\n\n").into_bytes();
        let first_boundary = first_message.len();
        let mut bytes = first_message.clone();
        if include_second {
            bytes.extend_from_slice(&second_message);
        }
        let input_len = bytes.len();
        (
            0usize..=input_len,
            proptest::collection::vec(0usize..=input_len, 0..=64),
        )
            .prop_map(move |(offset, cuts)| {
                let mut expected = Vec::new();
                if offset >= first_boundary {
                    expected.push(Outcome::Record(SseRecord::Dispatch(SseMessage {
                        event: None,
                        data: first.clone(),
                        last_event_id: None,
                    })));
                }
                if include_second && offset == input_len {
                    expected.push(Outcome::Record(SseRecord::Dispatch(SseMessage {
                        event: None,
                        data: second.clone(),
                        last_event_id: None,
                    })));
                }
                (
                    GeneratedCase {
                        bytes: bytes.clone(),
                        line_limit: 64,
                        frame_limit: 128,
                        cuts,
                        error_offset: Some(offset),
                    },
                    expected,
                )
            })
    });
    runner
        .run(&strategy, |(case, mut expected)| {
            let offset = case.error_offset.expect("generated logical error offset");
            expected.push(Outcome::Error(HttpErrorKind::Transport));
            let run = |cuts: &[usize]| {
                decode_stream(
                    &runtime,
                    stream_with_error(&case, cuts, offset),
                    case.line_limit,
                    case.frame_limit,
                )
            };
            let baseline = run(&[]);
            prop_assert_eq!(baseline, expected.clone());
            prop_assert_eq!(run(&case.cuts), expected.clone());
            prop_assert_eq!(run(&(1..offset).collect::<Vec<_>>()), expected);
            Ok(())
        })
        .expect("generated transport error layout invariant");
}

#[test]
fn test_generated_independent_field_semantics_cover_cursor_retry_bom_and_unknown_fields() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let examples: &[&[u8]] = &[
        b"\xef\xbb\xbfdata: a\rdata: b\r\r",
        b"retry: 9\nid: old\n\ndata: x\n\nid:\ndata: y\n\n",
        b"id: bad\0id\ndata: x\n\n",
        b"data: x\n\xef\xbb\xbfdata: y\n\n",
        b"x: ignored\n: comment\nretry: +1\nretry:\nretry: 18446744073709551616\ndata: z\n\n",
    ];
    for bytes in examples {
        let case = GeneratedCase {
            bytes: bytes.to_vec(),
            line_limit: 128,
            frame_limit: 512,
            cuts: Vec::new(),
            error_offset: None,
        };
        let output = decode(&runtime, &case, &[]);
        if *bytes == b"\xef\xbb\xbfdata: a\rdata: b\r\r" {
            assert_eq!(
                output,
                vec![Outcome::Record(SseRecord::Dispatch(SseMessage {
                    event: None,
                    data: "a\nb".to_owned(),
                    last_event_id: None
                }))]
            );
        } else if *bytes == b"retry: 9\nid: old\n\ndata: x\n\nid:\ndata: y\n\n" {
            assert_eq!(
                output,
                vec![
                    Outcome::Record(SseRecord::Control(SseControl::ReconnectDelayMs(9))),
                    Outcome::Record(SseRecord::Control(SseControl::LastEventId("old".to_owned()))),
                    Outcome::Record(SseRecord::Dispatch(SseMessage {
                        event: None,
                        data: "x".to_owned(),
                        last_event_id: Some("old".to_owned())
                    })),
                    Outcome::Record(SseRecord::Dispatch(SseMessage {
                        event: None,
                        data: "y".to_owned(),
                        last_event_id: Some(String::new())
                    }))
                ]
            );
        } else if *bytes == b"id: bad\0id\ndata: x\n\n" || *bytes == b"data: x\n\xef\xbb\xbfdata: y\n\n" {
            assert_eq!(
                output,
                vec![Outcome::Record(SseRecord::Dispatch(SseMessage {
                    event: None,
                    data: "x".to_owned(),
                    last_event_id: None
                }))]
            );
        } else {
            assert_eq!(
                output,
                vec![Outcome::Record(SseRecord::Dispatch(SseMessage {
                    event: None,
                    data: "z".to_owned(),
                    last_event_id: None
                }))]
            );
        }
    }
    let crlf = GeneratedCase {
        bytes: b"data: crlf\r\n\r\n".to_vec(),
        line_limit: 64,
        frame_limit: 64,
        cuts: vec![10],
        error_offset: None,
    };
    assert_eq!(
        decode(&runtime, &crlf, &[]),
        vec![Outcome::Record(SseRecord::Dispatch(SseMessage {
            event: None,
            data: "crlf".to_owned(),
            last_event_id: None
        }))]
    );
}
