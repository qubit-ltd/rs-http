/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Request body variants.

use bytes::Bytes;

/// Encodes how the outbound body is represented before sending via reqwest.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum HttpRequestBody {
    /// No body (typical for GET/HEAD).
    #[default]
    Empty,
    /// Opaque binary payload.
    Bytes(Bytes),
    /// UTF-8 text; builders may set `Content-Type: text/plain`.
    Text(String),
    /// JSON-serialized bytes; builders may set `Content-Type: application/json`.
    Json(Bytes),
}
