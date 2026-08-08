// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Reqwest transport phase markers used for error classification.

use parse_display::Display;
use parse_display::FromStr as DeriveFromStr;
use serde::Deserialize;
use serde::Serialize;

/// Phase where a [`reqwest::Error`] happened.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    Display,
    DeriveFromStr,
)]
#[serde(rename_all = "snake_case")]
#[display(style = "snake_case")]
pub(crate) enum ReqwestErrorPhase {
    /// Error happened while sending request / waiting first response bytes.
    Send,
    /// Error happened while reading response body.
    Read,
}
