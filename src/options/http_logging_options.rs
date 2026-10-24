// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_argument::ArgumentResult;
use qubit_argument::require_that;
use qubit_config::ConfigReader;
use qubit_config::ConfigResult;

use super::HttpConfigError;
use super::from_config_helpers::get_optional_usize;
use super::internal::HttpLoggingConfigInput;
use crate::constants::DEFAULT_LOG_BODY_SIZE_LIMIT_BYTES;

/// Controls TRACE-level HTTP request/response logging in [`crate::HttpLogger`].
///
/// # Examples
///
/// ```
/// use qubit_http::HttpLoggingOptions;
///
/// let mut options = HttpLoggingOptions::default();
/// options.log_request_body = false;
/// options.log_response_body = false;
/// options.body_size_limit = 0;
/// assert!(options.validate().is_ok());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpLoggingOptions {
    /// Whether logging is enabled.
    pub enabled: bool,
    /// Whether request headers are logged.
    pub log_request_header: bool,
    /// Whether request body is logged.
    pub log_request_body: bool,
    /// Whether response headers are logged.
    pub log_response_header: bool,
    /// Whether response body is logged.
    pub log_response_body: bool,
    /// Maximum body bytes to print in logs.
    pub body_size_limit: usize,
}

impl Default for HttpLoggingOptions {
    /// Enables logging of headers and bodies; body preview size is
    /// [`crate::constants::DEFAULT_LOG_BODY_SIZE_LIMIT_BYTES`].
    ///
    /// # Returns
    /// Default [`HttpLoggingOptions`].
    fn default() -> Self {
        Self {
            enabled: true,
            log_request_header: true,
            log_request_body: true,
            log_response_header: true,
            log_response_body: true,
            body_size_limit: DEFAULT_LOG_BODY_SIZE_LIMIT_BYTES,
        }
    }
}

/// Reads optional logging settings without applying defaults.
///
/// The caller merges the returned values into [`HttpLoggingOptions::default`]
/// and validates the completed options.
fn read_logging_config<R>(config: &R) -> ConfigResult<HttpLoggingConfigInput>
where
    R: ConfigReader + ?Sized,
{
    super::from_config_helpers::ensure_known_config_keys(
        config,
        &[
            "enabled",
            "log_request_header",
            "log_request_body",
            "log_response_header",
            "log_response_body",
            "body_size_limit",
        ],
        &[],
    )?;
    Ok(HttpLoggingConfigInput {
        enabled: config.get_optional("enabled")?,
        log_request_header: config.get_optional("log_request_header")?,
        log_request_body: config.get_optional("log_request_body")?,
        log_response_header: config.get_optional("log_response_header")?,
        log_response_body: config.get_optional("log_response_body")?,
        body_size_limit: get_optional_usize(config, "body_size_limit")?,
    })
}

impl HttpLoggingOptions {
    /// Reads logging settings from `config` using **relative** keys.
    ///
    /// # Parameters
    /// - `config`: Any [`ConfigReader`] (e.g. `config.section("logging")`).
    ///
    /// Keys read (all optional; missing keys keep their defaults):
    /// - `enabled`
    /// - `log_request_header`
    /// - `log_request_body`
    /// - `log_response_header`
    /// - `log_response_body`
    /// - `body_size_limit`
    ///
    /// # Returns
    /// Populated [`HttpLoggingOptions`] or [`HttpConfigError`].
    ///
    /// # Errors
    /// Returns an error when a key is invalid, unknown, or the completed
    /// options violate the body logging limit constraint.
    pub fn from_config<R>(config: &R) -> Result<Self, HttpConfigError>
    where
        R: ConfigReader + ?Sized,
    {
        let raw = read_logging_config(config).map_err(HttpConfigError::from)?;

        let mut opts = HttpLoggingOptions::default();
        if let Some(v) = raw.enabled {
            opts.enabled = v;
        }
        if let Some(v) = raw.log_request_header {
            opts.log_request_header = v;
        }
        if let Some(v) = raw.log_request_body {
            opts.log_request_body = v;
        }
        if let Some(v) = raw.log_response_header {
            opts.log_response_header = v;
        }
        if let Some(v) = raw.log_response_body {
            opts.log_response_body = v;
        }
        if let Some(v) = raw.body_size_limit {
            opts.body_size_limit = v;
        }

        opts.validate()
            .map_err(|error| super::from_config_helpers::resolve_component_error(config, error, "logging"))?;
        Ok(opts)
    }

    /// Ensures `body_size_limit` is non-zero when any body logging flag is
    /// enabled.
    ///
    /// # Returns
    /// `Ok(())` or [`HttpConfigError::invalid_value`] for
    /// `logging.body_size_limit`.
    ///
    /// # Errors
    /// Returns [`HttpConfigError`] when body logging is enabled and
    /// `body_size_limit` is zero.
    #[inline]
    pub fn validate(&self) -> Result<(), HttpConfigError> {
        self.validate_arguments().map_err(HttpConfigError::from)
    }

    /// Validates body logging limits as structured argument failures.
    ///
    /// # Returns
    /// `Ok(())` when body logging is disabled or its size limit is non-zero.
    ///
    /// # Errors
    /// Returns an [`ArgumentError`](qubit_argument::ArgumentError) when body
    /// logging is enabled and `body_size_limit` is zero.
    pub(super) fn validate_arguments(&self) -> ArgumentResult<()> {
        if self.log_request_body || self.log_response_body {
            require_that(
                self.body_size_limit,
                "logging.body_size_limit",
                |value| *value > 0,
                "body_logging_limit",
                "body_size_limit must be greater than 0 when body logging is enabled",
            )?;
        }
        Ok(())
    }
}
