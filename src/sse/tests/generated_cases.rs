// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded generated inputs and byte chunk layouts for SSE properties.

use bytes::Bytes;
use proptest::collection::vec;
use proptest::prelude::Just;
use proptest::prelude::Strategy;
use proptest::prelude::any;

use crate::HttpByteStream;

#[derive(Clone, Debug)]
pub(super) struct GeneratedCase {
    pub(super) bytes: Vec<u8>,
    pub(super) line_limit: usize,
    pub(super) frame_limit: usize,
    pub(super) cuts: Vec<usize>,
    pub(super) error_offset: Option<usize>,
}

pub(super) fn case_strategy() -> impl Strategy<Value = GeneratedCase> {
    let character = any::<char>().prop_map(|character| match character {
        '\r' | '\n' => 'x',
        character => character,
    });
    let value = vec(character, 0..=32).prop_map(|chars| chars.into_iter().collect::<String>());
    let line = proptest::prop_oneof![
        value.clone().prop_map(|value| format!("data: {value}")),
        value.clone().prop_map(|value| format!("event: {value}")),
        value.clone().prop_map(|value| format!("id: {value}")),
        proptest::prop_oneof![
            Just("retry: 7".to_owned()),
            Just("retry: 18446744073709551616".to_owned()),
            Just("retry: +1".to_owned()),
            Just("retry:".to_owned())
        ],
        value.clone().prop_map(|value| format!(":{value}")),
        value.prop_map(|value| format!("x-{value}")),
    ];
    (
        vec(line, 0..=30),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        1usize..=128,
        1usize..=512,
    )
        .prop_flat_map(
            |(lines, bom, eof_blank, malformed, terminate_last, line_limit, frame_limit)| {
                let ending = proptest::prop_oneof![Just("\n"), Just("\r"), Just("\r\n")];
                (vec(ending, lines.len().saturating_add(1)), vec(0usize..=8192, 0..=64)).prop_map(
                    move |(endings, mut cuts)| {
                        let mut bytes = Vec::new();
                        if bom {
                            bytes.extend_from_slice("\u{feff}".as_bytes());
                        }
                        for (index, line) in lines.iter().enumerate() {
                            bytes.extend_from_slice(line.as_bytes());
                            if index + 1 < lines.len() || terminate_last {
                                bytes.extend_from_slice(endings[index % endings.len()].as_bytes());
                            }
                        }
                        if eof_blank && !lines.is_empty() {
                            bytes.extend_from_slice(endings[lines.len() % endings.len()].as_bytes());
                        }
                        if malformed {
                            bytes.extend_from_slice(&[0xff, 0xfe]);
                        }
                        cuts.retain(|cut| *cut > 0 && *cut < bytes.len());
                        cuts.sort_unstable();
                        cuts.dedup();
                        let error_offset = (!bytes.is_empty()).then_some(bytes.len() / 2);
                        GeneratedCase {
                            bytes,
                            line_limit,
                            frame_limit,
                            cuts,
                            error_offset,
                        }
                    },
                )
            },
        )
}

pub(super) fn stream_for_case(case: &GeneratedCase, layout: &[usize]) -> HttpByteStream {
    let mut cuts = layout.to_vec();
    cuts.retain(|cut| *cut > 0 && *cut < case.bytes.len());
    cuts.sort_unstable();
    cuts.dedup();
    let mut chunks = Vec::with_capacity(cuts.len() * 2 + 3);
    chunks.push(Ok(Bytes::new()));
    let mut start = 0;
    for (index, end) in cuts.into_iter().chain(std::iter::once(case.bytes.len())).enumerate() {
        chunks.push(Ok(Bytes::copy_from_slice(&case.bytes[start..end])));
        for _ in 0..=index % 2 {
            chunks.push(Ok(Bytes::new()));
        }
        start = end;
    }
    Box::pin(futures_util::stream::iter(chunks))
}

pub(super) fn stream_with_error(case: &GeneratedCase, layout: &[usize], offset: usize) -> HttpByteStream {
    let offset = offset.min(case.bytes.len());
    let mut cuts = layout.to_vec();
    cuts.push(offset);
    cuts.retain(|cut| *cut < case.bytes.len());
    cuts.sort_unstable();
    cuts.dedup();
    let mut chunks = Vec::with_capacity(cuts.len() * 2 + 4);
    let mut start = 0;
    let mut injected = false;
    if offset == 0 {
        chunks.push(Err(crate::HttpError::transport("generated transport failure")));
        injected = true;
    }
    for (index, end) in cuts.into_iter().chain(std::iter::once(case.bytes.len())).enumerate() {
        chunks.push(Ok(Bytes::copy_from_slice(&case.bytes[start..end])));
        start = end;
        if !injected && start >= offset {
            chunks.push(Err(crate::HttpError::transport("generated transport failure")));
            injected = true;
        }
        if index < 2 {
            chunks.push(Ok(Bytes::new()));
        }
    }
    if !injected {
        chunks.push(Err(crate::HttpError::transport("generated transport failure")));
    }
    Box::pin(futures_util::stream::iter(chunks))
}
