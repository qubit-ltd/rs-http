// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Deterministic chunk layouts used by SSE decoder tests.

use std::ops::Range;

use bytes::Bytes;

use crate::HttpByteStream;

/// Builds deterministic contiguous layouts using the line or frame test
/// profile.
pub(crate) fn chunk_layouts(input: &[u8], frame_profile: bool) -> Vec<Vec<Range<usize>>> {
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
    let (mut state, multiplier, increment) = if frame_profile {
        (0xd1b5_4a32_d192_ed03u64, 2_862_933_555_777_941_757, 3_037_000_493)
    } else {
        (
            0x9e37_79b9_7f4a_7c15u64,
            6_364_136_223_846_793_005,
            1_442_695_040_888_963_407,
        )
    };
    while layouts.len() < fixed_layout_count + 64 {
        let mut cuts = vec![0];
        for offset in 1..len {
            state = state.wrapping_mul(multiplier).wrapping_add(increment);
            let should_cut = if frame_profile {
                state % 7 < 2
            } else {
                state.is_multiple_of(5)
            };
            if should_cut {
                cuts.push(offset);
            }
        }
        cuts.push(len);
        layouts.push(cuts.windows(2).map(|pair| pair[0]..pair[1]).collect());
    }
    layouts
}

/// Builds a byte stream with the exact empty-chunk pattern used by SSE tests.
pub(crate) fn stream_for_layout(input: &[u8], layout: &[Range<usize>]) -> HttpByteStream {
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
