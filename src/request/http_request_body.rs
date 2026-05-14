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
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Display, EnumString)]
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
    /// Multipart body bytes; builders may set a default `multipart/form-data` Content-Type.
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
