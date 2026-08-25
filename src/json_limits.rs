// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared JSON resource profiles for HTTP response and SSE decoding.

use qubit_budget::json::JsonDecodeLimits;
use qubit_budget::json::JsonEncodeLimits;
use qubit_budget::json::JsonValueLimits;

/// Maximum expansion of one raw control byte into a `\u00XX` JSON escape.
const MAX_NORMALIZED_BYTE_EXPANSION: usize = 6;

/// Builds the default JSON value limits for untrusted HTTP payloads.
///
/// # Returns
///
/// The default structural and payload profile shared by whole-response JSON,
/// SSE JSON, and JSON-valued HTTP configuration.
#[must_use]
pub(crate) fn default_json_value_limits() -> JsonValueLimits {
    JsonValueLimits::builder()
        .max_depth(128)
        .max_nodes(100_000)
        .max_sequence_items(100_000)
        .max_map_entries(100_000)
        .max_key_bytes(64 * 1024)
        .max_string_bytes(8 * 1024 * 1024)
        .max_number_bytes(4 * 1024)
        .max_payload_bytes(8 * 1024 * 1024)
        .build()
}

/// Builds the default JSON encoding limits for outbound HTTP request bodies.
///
/// # Returns
///
/// The standard JSON value profile plus an eight-mebibyte encoded-body limit.
#[must_use]
pub(crate) fn default_json_encode_limits() -> JsonEncodeLimits {
    JsonEncodeLimits::builder()
        .max_output_bytes(8 * 1024 * 1024)
        .value_limits(default_json_value_limits())
        .build()
}

/// Combines a transport byte boundary with JSON value limits.
///
/// The normalized-input allowance covers the worst-case expansion of every
/// raw byte into a six-byte Unicode escape without weakening the raw transport
/// boundary.
///
/// # Parameters
///
/// * `max_input_bytes` - Maximum raw payload bytes admitted by the transport
///   boundary.
/// * `value_limits` - Structural and decoded-payload limits.
///
/// # Returns
///
/// Complete limits for one normalizing JSON decoder.
#[must_use]
pub(crate) fn json_decode_limits(
    max_input_bytes: usize,
    value_limits: JsonValueLimits,
) -> JsonDecodeLimits {
    JsonDecodeLimits::builder()
        .max_input_bytes(max_input_bytes)
        .max_normalized_input_bytes(
            max_input_bytes.saturating_mul(MAX_NORMALIZED_BYTE_EXPANSION),
        )
        .value_limits(value_limits)
        .build()
}
