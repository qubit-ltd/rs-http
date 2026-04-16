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
use parse_display::FromStr as DeriveFromStr;

/// How to handle JSON parse failures on SSE `data:` lines.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    DeriveFromStr,
)]
#[serde(rename_all = "snake_case")]
#[display(style = "snake_case")]
pub enum SseJsonMode {
    /// Skip bad chunks and continue.
    #[from_str(regex = "(?i)lenient")]
    Lenient,
    /// Propagate [`crate::HttpError::sse_decode`] on first failure.
    #[from_str(regex = "(?i)strict")]
    Strict,
}
