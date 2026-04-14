/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # SSE event record
//!
//! One dispatch after frame reassembly (`data:` lines joined with `\n`).
//!
//! # Author
//!
//! Haixing Hu

use serde::de::DeserializeOwned;

use super::SseJsonMode;
use crate::{HttpError, HttpResult};

/// One Server-Sent Events dispatch after frame reassembly (`data:` lines joined with `\n`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// `event:` field if present.
    pub event: Option<String>,
    /// Concatenated `data:` payload (newline-separated if multiple `data` lines).
    pub data: String,
    /// `id:` field if present.
    pub id: Option<String>,
    /// Parsed `retry:` milliseconds hint if valid.
    pub retry: Option<u64>,
}

impl SseEvent {
    /// Decodes the current event's `data` payload as JSON.
    ///
    /// # Type parameters
    /// - `T`: Target type deserialized from [`SseEvent::data`].
    ///
    /// # Returns
    /// `Ok(T)` when `data` is valid JSON for `T`.
    ///
    /// # Errors
    /// Returns [`HttpError::sse_decode`] when JSON parsing fails.
    /// The error message includes optional `event` and `id` context.
    pub fn decode_json<T>(&self) -> HttpResult<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_str::<T>(&self.data).map_err(|error| {
            HttpError::sse_decode(format!(
                "Failed to decode SSE event data as JSON (event={:?}, id={:?}): {}",
                self.event, self.id, error
            ))
        })
    }

    /// Decodes the current event's `data` payload as JSON with configurable strictness.
    ///
    /// # Parameters
    /// - `mode`: JSON decoding strictness.
    ///
    /// # Type parameters
    /// - `T`: Target type deserialized from [`SseEvent::data`].
    ///
    /// # Returns
    /// - `Ok(Some(T))` when `data` is valid JSON for `T`.
    /// - `Ok(None)` in lenient mode when JSON parsing fails.
    ///
    /// # Errors
    /// Returns [`HttpError::sse_decode`] in strict mode when JSON parsing fails.
    pub fn decode_json_with_mode<T>(&self, mode: SseJsonMode) -> HttpResult<Option<T>>
    where
        T: DeserializeOwned,
    {
        match mode {
            SseJsonMode::Strict => self.decode_json::<T>().map(Some),
            SseJsonMode::Lenient => match serde_json::from_str::<T>(&self.data) {
                Ok(value) => Ok(Some(value)),
                Err(error) => {
                    tracing::debug!(
                        "Skipping malformed SSE event JSON in lenient mode (event={:?}, id={:?}): {}",
                        self.event,
                        self.id,
                        error
                    );
                    Ok(None)
                }
            },
        }
    }
}
