// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Strictness for JSON parsing on SSE `data:` lines.

use parse_display::FromStr as DeriveFromStr;
use serde::{Deserialize, Serialize};

/// How to handle JSON parse failures on SSE `data:` lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, DeriveFromStr)]
#[serde(rename_all = "snake_case")]
#[display(style = "snake_case")]
pub enum SseJsonMode {
    /// Apply supported JSON normalization, then skip chunks that remain
    /// invalid.
    #[from_str(regex = "(?i)lenient")]
    Lenient,
    /// Propagate [`crate::HttpError::sse_decode`] on first failure.
    #[from_str(regex = "(?i)strict")]
    Strict,
}
