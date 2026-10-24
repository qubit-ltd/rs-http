// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Decoded JSON chunk or explicit SSE stream end marker.

use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use strum::Display;
use strum::EnumString;

/// Represents a decoded value from one SSE data payload or an explicit end
/// marker emitted by the decoder.
///
/// # Type Parameters
///
/// * `T` - The type produced by deserializing an SSE data payload.
///
/// # Examples
///
/// ```
/// use qubit_http::sse::SseChunk;
///
/// let chunk = SseChunk::Data("connected".to_owned());
/// assert_eq!(chunk, SseChunk::Data("connected".to_owned()));
/// assert_eq!(SseChunk::<String>::Done, SseChunk::Done);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Display, EnumString)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: DeserializeOwned"))]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SseChunk<T> {
    /// A value successfully deserialized from an SSE data payload.
    #[strum(disabled)]
    Data(T),
    /// A synthetic item emitted when
    /// [`DoneMarkerPolicy`](crate::sse::DoneMarkerPolicy) matches; the stream
    /// then ends.
    Done,
}
