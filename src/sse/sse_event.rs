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
