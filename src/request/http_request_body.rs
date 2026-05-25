/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Request body variants.

use std::fmt;

use bytes::Bytes;
use serde::{
    Deserialize,
    Serialize,
};
use strum::{
    Display,
    EnumString,
};

/// Encodes how the outbound body is represented before sending via reqwest.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum HttpRequestBody {
    /// No body (typical for GET/HEAD).
    #[default]
    Empty,
    /// Opaque binary payload.
    #[strum(disabled)]
    Bytes(Bytes),
    /// UTF-8 text; builders may set `Content-Type: text/plain`.
    #[strum(disabled)]
    Text(String),
    /// JSON-serialized bytes; builders may set `Content-Type: application/json`.
    #[strum(disabled)]
    Json(Bytes),
    /// URL-encoded form bytes; builders may set `Content-Type: application/x-www-form-urlencoded`.
    #[strum(disabled)]
    Form(Bytes),
    /// Multipart body bytes; builders ensure multipart Content-Type boundary consistency.
    #[strum(disabled)]
    Multipart(Bytes),
    /// NDJSON bytes (`\n`-delimited JSON objects); builders may set `application/x-ndjson`.
    #[strum(disabled)]
    Ndjson(Bytes),
    /// Chunked upload payload represented as ordered byte chunks.
    ///
    /// The client sends this variant through reqwest streaming body support.
    #[strum(disabled)]
    Stream(Vec<Bytes>),
}

impl fmt::Debug for HttpRequestBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("Empty"),
            Self::Bytes(bytes) => formatter.debug_struct("Bytes").field("len", &bytes.len()).finish(),
            Self::Text(text) => formatter.debug_struct("Text").field("len", &text.len()).finish(),
            Self::Json(bytes) => formatter.debug_struct("Json").field("len", &bytes.len()).finish(),
            Self::Form(bytes) => formatter.debug_struct("Form").field("len", &bytes.len()).finish(),
            Self::Multipart(bytes) => formatter.debug_struct("Multipart").field("len", &bytes.len()).finish(),
            Self::Ndjson(bytes) => formatter.debug_struct("Ndjson").field("len", &bytes.len()).finish(),
            Self::Stream(chunks) => formatter
                .debug_struct("Stream")
                .field("chunks", &chunks.len())
                .field("len", &chunks.iter().map(Bytes::len).sum::<usize>())
                .finish(),
        }
    }
}
