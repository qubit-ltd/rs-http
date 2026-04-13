/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use qubit_config::{ConfigReader, ConfigResult};

use super::HttpConfigError;
use crate::constants::DEFAULT_LOG_BODY_SIZE_LIMIT_BYTES;

/// Controls TRACE-level HTTP request/response logging in [`crate::HttpLogger`].
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

/// Raw optional fields read from config before merging into [`HttpLoggingOptions`] defaults.
struct LoggingConfigInput {
    enabled: Option<bool>,
    log_request_header: Option<bool>,
    log_request_body: Option<bool>,
    log_response_header: Option<bool>,
    log_response_body: Option<bool>,
    body_size_limit: Option<usize>,
}

fn read_logging_config<R: ConfigReader + ?Sized>(config: &R) -> ConfigResult<LoggingConfigInput> {
    Ok(LoggingConfigInput {
        enabled: config.get_optional("enabled")?,
        log_request_header: config.get_optional("log_request_header")?,
        log_request_body: config.get_optional("log_request_body")?,
        log_response_header: config.get_optional("log_response_header")?,
        log_response_body: config.get_optional("log_response_body")?,
        body_size_limit: config.get_optional("body_size_limit")?,
    })
}

impl HttpLoggingOptions {
    /// Reads logging settings from `config` using **relative** keys.
    ///
    /// # Parameters
    /// - `config`: Any [`ConfigReader`] (e.g. `config.prefix_view("logging")`).
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
    pub fn from_config<R: ConfigReader + ?Sized>(config: &R) -> Result<Self, HttpConfigError> {
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

        Ok(opts)
    }

    /// Ensures `body_size_limit` is non-zero when any body logging flag is enabled.
    ///
    /// # Returns
    /// `Ok(())` or [`HttpConfigError::invalid_value`] for `logging.body_size_limit`.
    pub fn validate(&self) -> Result<(), HttpConfigError> {
        if (self.log_request_body || self.log_response_body) && self.body_size_limit == 0 {
            return Err(HttpConfigError::invalid_value(
                "logging.body_size_limit",
                "body_size_limit must be greater than 0 when body logging is enabled",
            ));
        }
        Ok(())
    }
}
