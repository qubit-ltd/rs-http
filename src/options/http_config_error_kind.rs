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

use std::fmt;

/// Category of HTTP configuration errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpConfigErrorKind {
    /// A required configuration key is missing.
    MissingField,
    /// The value exists but cannot be converted to the expected type.
    TypeError,
    /// The value is present and well-typed but semantically invalid.
    InvalidValue,
    /// A header name or value cannot be converted to an HTTP header.
    InvalidHeader,
    /// An underlying `qubit-config` error occurred.
    ConfigError,
}

impl fmt::Display for HttpConfigErrorKind {
    /// Writes a short human-readable label for this error kind.
    ///
    /// # Parameters
    /// - `f`: Destination formatter.
    ///
    /// # Returns
    /// [`fmt::Result`] from the write operations.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpConfigErrorKind::MissingField => write!(f, "missing field"),
            HttpConfigErrorKind::TypeError => write!(f, "type error"),
            HttpConfigErrorKind::InvalidValue => write!(f, "invalid value"),
            HttpConfigErrorKind::InvalidHeader => write!(f, "invalid header"),
            HttpConfigErrorKind::ConfigError => write!(f, "config error"),
        }
    }
}
