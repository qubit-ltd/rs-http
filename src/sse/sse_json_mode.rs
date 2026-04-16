/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Strictness for JSON parsing on SSE `data:` lines.
//!
//! # Author
//!
//! Haixing Hu

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// How to handle JSON parse failures on SSE `data:` lines.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    Display,
    EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(ascii_case_insensitive, serialize_all = "snake_case")]
pub enum SseJsonMode {
    /// Skip bad chunks and continue.
    Lenient,
    /// Propagate [`crate::HttpError::sse_decode`] on first failure.
    Strict,
}
