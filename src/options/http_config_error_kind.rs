/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # HTTP configuration error kind
//!
//! Category enum for [`crate::HttpConfigError`].
//!
//! # Author
//!
//! Haixing Hu

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Category of HTTP configuration errors.
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
pub enum HttpConfigErrorKind {
    /// A required configuration key is missing.
    #[strum(to_string = "missing field")]
    MissingField,
    /// The value exists but cannot be converted to the expected type.
    #[strum(to_string = "type error")]
    TypeError,
    /// The value is present and well-typed but semantically invalid.
    #[strum(to_string = "invalid value")]
    InvalidValue,
    /// A header name or value cannot be converted to an HTTP header.
    #[strum(to_string = "invalid header")]
    InvalidHeader,
    /// An underlying `qubit-config` error occurred.
    #[strum(to_string = "config error")]
    ConfigError,
}
