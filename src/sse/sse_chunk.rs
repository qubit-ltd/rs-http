/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Decoded JSON chunk or explicit SSE stream end marker.
//!
//! # Author
//!
//! Haixing Hu

/// Either a decoded JSON value from one SSE data payload or an explicit end marker.
#[derive(Debug, Clone, PartialEq)]
pub enum SseChunk<T> {
    /// Successfully deserialized JSON object.
    Data(T),
    /// Synthetic item emitted when [`DoneMarkerPolicy`](crate::sse::DoneMarkerPolicy) matches (then stream ends).
    Done,
}
