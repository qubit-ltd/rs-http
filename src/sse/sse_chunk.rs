/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Decoded JSON chunk or explicit SSE stream end marker.
//!

use serde::{
    Deserialize,
    Serialize,
};
use strum::{
    Display,
    EnumString,
};

/// Either a decoded JSON value from one SSE data payload or an explicit end marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Display, EnumString)]
#[serde(bound(
    serialize = "T: serde::Serialize",
    deserialize = "T: serde::de::DeserializeOwned"
))]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SseChunk<T> {
    /// Successfully deserialized JSON object.
    #[strum(disabled)]
    Data(T),
    /// Synthetic item emitted when [`DoneMarkerPolicy`](crate::sse::DoneMarkerPolicy)
    /// matches (then stream ends).
    Done,
}
