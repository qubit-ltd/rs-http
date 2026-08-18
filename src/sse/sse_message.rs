// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # SSE message record
//!
//! One EventSource-style message dispatch after frame reassembly.
use qubit_json::decode::NormalizingJsonDecodeOptions;
use qubit_json::decode::NormalizingJsonDecoder;
use serde::de::DeserializeOwned;

use super::SseJsonMode;
use crate::HttpError;
use crate::HttpResult;

/// One EventSource-style message dispatch after `data:` line reassembly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseMessage {
    /// `event:` field if present; callers may treat `None` as the default
    /// `message` type.
    pub event: Option<String>,
    /// Concatenated `data:` payload.
    pub data: String,
    /// Last event id after applying this frame and any prior control-only
    /// `id:` frames.
    pub last_event_id: Option<String>,
}

impl SseMessage {
    /// Decodes the current message's `data` payload as JSON.
    ///
    /// # Type parameters
    /// - `T`: Target type deserialized from [`SseMessage::data`].
    ///
    /// # Returns
    /// `Ok(T)` when `data` is valid JSON for `T`.
    ///
    /// # Errors
    /// Returns [`HttpError::sse_decode`] when JSON parsing fails.
    pub fn decode_json<T>(&self) -> HttpResult<T>
    where
        T: DeserializeOwned,
    {
        NormalizingJsonDecoder::new(NormalizingJsonDecodeOptions::strict())
            .decode_str::<T>(&self.data)
            .map_err(|error| {
                HttpError::sse_decode(format!(
                    "Failed to decode SSE message data as JSON (event={:?}, last_event_id={:?})",
                    self.event, self.last_event_id
                ))
                .with_source(error)
            })
    }

    /// Decodes the current message's `data` payload as JSON with configurable
    /// strictness.
    ///
    /// # Parameters
    /// - `mode`: JSON decoding strictness.
    ///
    /// # Type parameters
    /// - `T`: Target type deserialized from [`SseMessage::data`].
    ///
    /// # Returns
    /// - `Ok(Some(T))` when `data` is valid JSON for `T`.
    /// - `Ok(Some(T))` in lenient mode when supported text normalization makes
    ///   `data` valid JSON for `T`.
    /// - `Ok(None)` in lenient mode when JSON remains invalid after
    ///   normalization.
    ///
    /// # Errors
    /// Returns [`HttpError::sse_decode`] in strict mode when JSON parsing
    /// fails.
    pub fn decode_json_with_mode<T>(
        &self,
        mode: SseJsonMode,
    ) -> HttpResult<Option<T>>
    where
        T: DeserializeOwned,
    {
        match mode {
            SseJsonMode::Strict => self.decode_json::<T>().map(Some),
            SseJsonMode::Lenient => {
                match NormalizingJsonDecoder::default()
                    .decode_str::<T>(&self.data)
                {
                    Ok(value) => Ok(Some(value)),
                    Err(error) => {
                        tracing::debug!(
                            error_kind = %error.kind(),
                            error_stage = %error.stage(),
                            normalized_line = ?error.normalized_line(),
                            normalized_column = ?error.normalized_column(),
                            event = ?self.event,
                            last_event_id = ?self.last_event_id,
                            "Skipping malformed SSE message JSON in lenient mode",
                        );
                        Ok(None)
                    }
                }
            }
        }
    }
}
