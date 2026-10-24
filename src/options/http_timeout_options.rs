// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use qubit_argument::require_that;
use qubit_argument::ArgumentResult;
use qubit_argument::OptionArgument;
use qubit_config::ConfigReader;
use qubit_config::ConfigResult;

use super::internal::TimeoutConfigInput;
use super::HttpConfigError;
use crate::constants::DEFAULT_CONNECT_TIMEOUT_SECS;
use crate::constants::DEFAULT_READ_TIMEOUT_SECS;
use crate::constants::DEFAULT_SEND_TIMEOUT_SECS;

/// Configures connect, read, response-header, and whole-request HTTP timeouts.
///
/// Every configured timeout must be greater than zero. The request timeout is
/// optional and bounds the overall duration of an individual request.
///
/// # Examples
///
/// ```
/// use qubit_http::HttpTimeoutOptions;
/// use std::time::Duration;
///
/// let mut timeouts = HttpTimeoutOptions::default();
/// timeouts.request_timeout = Some(Duration::from_secs(30));
/// assert!(timeouts.validate().is_ok());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpTimeoutOptions {
    /// Maximum time allowed to establish a connection; must be nonzero.
    pub connect_timeout: Duration,
    /// Maximum wait for a read operation; must be nonzero.
    pub read_timeout: Duration,
    /// Maximum wait for response headers; must be nonzero.
    pub response_header_timeout: Duration,
    /// Optional maximum duration for an entire request; when set, must be
    /// nonzero.
    pub request_timeout: Option<Duration>,
}

impl Default for HttpTimeoutOptions {
    /// Connect / read / write durations use
    /// [`crate::constants::DEFAULT_CONNECT_TIMEOUT_SECS`],
    /// [`crate::constants::DEFAULT_READ_TIMEOUT_SECS`], and
    /// [`crate::constants::DEFAULT_SEND_TIMEOUT_SECS`]; no global request
    /// timeout.
    ///
    /// # Returns
    /// Default [`HttpTimeoutOptions`].
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS),
            read_timeout: Duration::from_secs(DEFAULT_READ_TIMEOUT_SECS),
            response_header_timeout: Duration::from_secs(DEFAULT_SEND_TIMEOUT_SECS),
            request_timeout: None,
        }
    }
}

/// Reads the supported timeout keys as optional durations.
///
/// Unknown keys are rejected by the shared configuration helper.
fn read_timeout_config<R>(config: &R) -> ConfigResult<TimeoutConfigInput>
where
    R: ConfigReader + ?Sized,
{
    super::from_config_helpers::ensure_known_config_keys(
        config,
        &[
            "connect_timeout",
            "read_timeout",
            "response_header_timeout",
            "request_timeout",
        ],
        &[],
    )?;
    Ok(TimeoutConfigInput {
        connect_timeout: config.get_optional("connect_timeout")?,
        read_timeout: config.get_optional("read_timeout")?,
        response_header_timeout: config.get_optional("response_header_timeout")?,
        request_timeout: config.get_optional("request_timeout")?,
    })
}

impl HttpTimeoutOptions {
    /// Validates timeout bounds.
    ///
    /// # Returns
    /// `Ok(())` when all configured durations are strictly greater than zero.
    ///
    /// # Errors
    /// Returns [`HttpConfigError`] identifying a timeout whose value is zero.
    pub fn validate(&self) -> Result<(), HttpConfigError> {
        self.validate_arguments().map_err(HttpConfigError::from)
    }

    /// Validates timeout values while retaining structured argument errors.
    ///
    /// # Errors
    /// Returns an argument error for the first timeout value that is zero.
    pub(super) fn validate_arguments(&self) -> ArgumentResult<()> {
        validate_positive_duration("connect_timeout", self.connect_timeout)?;
        validate_positive_duration("read_timeout", self.read_timeout)?;
        validate_positive_duration("response_header_timeout", self.response_header_timeout)?;
        self.request_timeout.validate_some(|request_timeout| {
            validate_positive_duration("request_timeout", request_timeout)
        })?;
        Ok(())
    }

    /// Reads timeout settings from `config` using **relative** keys.
    ///
    /// # Parameters
    /// - `config`: Any [`ConfigReader`] (e.g. root [`qubit_config::Config`] or
    ///   `config.section("timeouts")`).
    ///
    /// Keys read (all optional; missing keys keep their defaults):
    /// - `connect_timeout`
    /// - `read_timeout`
    /// - `response_header_timeout`
    /// - `request_timeout`
    ///
    /// # Returns
    /// Populated [`HttpTimeoutOptions`] or [`HttpConfigError`] on type
    /// conversion failure.
    ///
    /// # Errors
    /// Returns [`HttpConfigError`] for unknown keys, invalid value types, or
    /// timeout values that fail validation.
    pub fn from_config<R>(config: &R) -> Result<Self, HttpConfigError>
    where
        R: ConfigReader + ?Sized,
    {
        let raw = read_timeout_config(config).map_err(HttpConfigError::from)?;

        let mut opts = HttpTimeoutOptions::default();
        if let Some(d) = raw.connect_timeout {
            opts.connect_timeout = d;
        }
        if let Some(d) = raw.read_timeout {
            opts.read_timeout = d;
        }
        if let Some(d) = raw.response_header_timeout {
            opts.response_header_timeout = d;
        }
        opts.request_timeout = raw.request_timeout;
        opts.validate()
            .map_err(|error| super::from_config_helpers::resolve_config_error(config, error))?;
        Ok(opts)
    }
}

/// Returns `value` when it is nonzero while preserving the public domain
/// diagnostic on failure.
///
/// # Parameters
/// - `path`: Configuration field name used in any validation error.
/// - `value`: Duration to validate.
///
/// # Returns
/// The input duration unchanged when it is greater than zero.
///
/// # Errors
/// Returns a structured argument error when `value` is zero.
fn validate_positive_duration(path: &str, value: Duration) -> ArgumentResult<Duration> {
    require_that(
        value,
        path,
        |duration| !duration.is_zero(),
        "positive_timeout",
        "Timeout value must be greater than 0",
    )
}
