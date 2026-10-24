// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Optional scalar overrides read from the `json` configuration section.
pub(super) struct HttpClientJsonConfigInput {
    /// Maximum nesting depth.
    pub(super) max_depth: Option<usize>,
    /// Maximum total JSON nodes.
    pub(super) max_nodes: Option<usize>,
    /// Maximum items in one array.
    pub(super) max_sequence_items: Option<usize>,
    /// Maximum entries in one object.
    pub(super) max_map_entries: Option<usize>,
    /// Maximum UTF-8 bytes in one object key.
    pub(super) max_key_bytes: Option<usize>,
    /// Maximum UTF-8 bytes in one string value.
    pub(super) max_string_bytes: Option<usize>,
    /// Maximum bytes in one number lexeme.
    pub(super) max_number_bytes: Option<usize>,
    /// Maximum cumulative key, string, and number payload bytes.
    pub(super) max_payload_bytes: Option<usize>,
    /// Maximum encoded JSON request-body bytes.
    pub(super) max_output_bytes: Option<usize>,
}
