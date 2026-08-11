// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # HTTP configuration error
//!
//! Error type for configuration-to-options conversion failures.

use std::fmt;

use qubit_argument::ArgumentErrorKind;

use super::HttpConfigErrorKind;

/// Error type for HTTP configuration conversion failures.
///
/// Carries the failing configuration path and a human-readable message so that
/// callers can report exactly which key caused the problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpConfigError {
    /// The configuration path that triggered the error, e.g.
    /// `http.proxy.port`.
    pub path: String,
    /// Human-readable description of the problem.
    pub message: String,
    /// Error category.
    pub kind: HttpConfigErrorKind,
}

impl HttpConfigError {
    /// Builds a configuration error with the given classification and message.
    ///
    /// # Parameters
    /// - `kind`: Error category.
    /// - `path`: Configuration key path (e.g. `http.proxy.port`).
    /// - `message`: Human-readable explanation.
    ///
    /// # Returns
    /// New [`HttpConfigError`].
    pub fn new(
        kind: HttpConfigErrorKind,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            path: path.into(),
            message: message.into(),
        }
    }

    /// Shorthand for [`HttpConfigErrorKind::MissingField`].
    ///
    /// # Parameters
    /// - `path`: Configuration path of the missing field.
    /// - `message`: Explanation of what is missing.
    ///
    /// # Returns
    /// New [`HttpConfigError`].
    pub fn missing(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(HttpConfigErrorKind::MissingField, path, message)
    }

    /// Shorthand for [`HttpConfigErrorKind::TypeError`].
    ///
    /// # Parameters
    /// - `path`: Configuration path where the type mismatch occurred.
    /// - `message`: Details of the expected vs actual type.
    ///
    /// # Returns
    /// New [`HttpConfigError`].
    pub fn type_error(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(HttpConfigErrorKind::TypeError, path, message)
    }

    /// Shorthand for [`HttpConfigErrorKind::InvalidValue`].
    ///
    /// # Parameters
    /// - `path`: Configuration path of the invalid value.
    /// - `message`: Why the value is not acceptable.
    ///
    /// # Returns
    /// New [`HttpConfigError`].
    pub fn invalid_value(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(HttpConfigErrorKind::InvalidValue, path, message)
    }

    /// Shorthand for [`HttpConfigErrorKind::InvalidHeader`].
    ///
    /// # Parameters
    /// - `path`: Configuration path related to the header map entry.
    /// - `message`: Header name/value problem description.
    ///
    /// # Returns
    /// New [`HttpConfigError`].
    pub fn invalid_header(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(HttpConfigErrorKind::InvalidHeader, path, message)
    }

    /// Shorthand for [`HttpConfigErrorKind::ConfigError`] (underlying
    /// `qubit-config` failure).
    ///
    /// # Parameters
    /// - `path`: Configuration path if known; may be empty when not applicable.
    /// - `message`: Error text from the config layer.
    ///
    /// # Returns
    /// New [`HttpConfigError`].
    pub fn config_error(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(HttpConfigErrorKind::ConfigError, path, message)
    }
}

impl fmt::Display for HttpConfigError {
    /// Formats as `[kind] path: message`.
    ///
    /// # Parameters
    /// - `f`: Destination formatter.
    ///
    /// # Returns
    /// [`fmt::Result`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.kind, self.path, self.message)
    }
}

impl std::error::Error for HttpConfigError {}

impl From<qubit_argument::ArgumentError> for HttpConfigError {
    /// Converts structured argument validation failures into invalid HTTP
    /// configuration values.
    ///
    /// The argument path is retained. Caller-defined validation messages are
    /// preserved without embedding the path; other structured kinds use a
    /// pathless fallback diagnostic.
    fn from(error: qubit_argument::ArgumentError) -> Self {
        let (kind, message) = match error.kind() {
            ArgumentErrorKind::Missing => (
                HttpConfigErrorKind::MissingField,
                "Required value is missing".to_owned(),
            ),
            ArgumentErrorKind::Custom { code, message } if code == "http_config_missing" => {
                (HttpConfigErrorKind::MissingField, message.clone())
            }
            ArgumentErrorKind::Custom { message, .. } => {
                (HttpConfigErrorKind::InvalidValue, message.clone())
            }
            _ => (
                HttpConfigErrorKind::InvalidValue,
                "Argument validation failed".to_owned(),
            ),
        };
        let path = error.path().as_str().to_owned();
        Self::new(kind, path, message)
    }
}

impl From<qubit_config::ConfigError> for HttpConfigError {
    /// Converts a `qubit_config::ConfigError`, mapping typed failures to
    /// [`HttpConfigErrorKind::TypeError`] when the source carries a property
    /// key.
    ///
    /// # Parameters
    /// - `e`: Source configuration error.
    ///
    /// # Returns
    /// Equivalent [`HttpConfigError`].
    fn from(e: qubit_config::ConfigError) -> Self {
        use qubit_config::ConfigErrorKind;
        let kind = e.kind();
        let path = e.path().unwrap_or_default().to_owned();
        let msg = e.to_string();
        match kind {
            ConfigErrorKind::TypeMismatch
            | ConfigErrorKind::Conversion
            | ConfigErrorKind::PropertyHasNoValue => HttpConfigError::type_error(path, msg),
            _ => HttpConfigError::config_error(path, msg),
        }
    }
}
