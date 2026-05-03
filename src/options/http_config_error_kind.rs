/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! # HTTP configuration error kind
//!
//! Category enum for [`crate::HttpConfigError`].
//!

use parse_display::{Display, FromStr as DeriveFromStr};
use serde::{Deserialize, Serialize};

/// Category of HTTP configuration errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, DeriveFromStr)]
#[serde(rename_all = "snake_case")]
#[display(style = "snake_case")]
pub enum HttpConfigErrorKind {
    /// A required configuration key is missing.
    #[display("missing field")]
    #[from_str(regex = "(?i)missing\\s+field")]
    MissingField,
    /// The value exists but cannot be converted to the expected type.
    #[display("type error")]
    #[from_str(regex = "(?i)type\\s+error")]
    TypeError,
    /// The value is present and well-typed but semantically invalid.
    #[display("invalid value")]
    #[from_str(regex = "(?i)invalid\\s+value")]
    InvalidValue,
    /// A header name or value cannot be converted to an HTTP header.
    #[display("invalid header")]
    #[from_str(regex = "(?i)invalid\\s+header")]
    InvalidHeader,
    /// An underlying `qubit-config` error occurred.
    #[display("config error")]
    #[from_str(regex = "(?i)config\\s+error")]
    ConfigError,
}
