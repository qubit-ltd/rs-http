// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Internal building blocks for safe diagnostic message rendering.

mod message_emitter;
mod message_segment;

pub(in crate::redact) use message_emitter::MessageEmitter;
pub(in crate::redact) use message_segment::MessageSegment;
