/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::Duration;

use qubit_config::{ConfigReader, ConfigResult};

use super::HttpConfigError;
use crate::constants::{
    DEFAULT_CONNECT_TIMEOUT_SECS, DEFAULT_READ_TIMEOUT_SECS, DEFAULT_WRITE_TIMEOUT_SECS,
};

/// Connect, read, write, and optional whole-request timeouts for HTTP I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeoutOptions {
    /// Connect timeout.
    pub connect_timeout: Duration,
    /// Read timeout.
    pub read_timeout: Duration,
    /// Write timeout.
    pub write_timeout: Duration,
    /// Optional global request timeout.
    pub request_timeout: Option<Duration>,
}

impl Default for TimeoutOptions {
    /// Connect / read / write durations use
    /// [`crate::constants::DEFAULT_CONNECT_TIMEOUT_SECS`],
    /// [`crate::constants::DEFAULT_READ_TIMEOUT_SECS`], and
    /// [`crate::constants::DEFAULT_WRITE_TIMEOUT_SECS`]; no global request timeout.
    ///
    /// # Returns
    /// Default [`TimeoutOptions`].
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS),
            read_timeout: Duration::from_secs(DEFAULT_READ_TIMEOUT_SECS),
            write_timeout: Duration::from_secs(DEFAULT_WRITE_TIMEOUT_SECS),
            request_timeout: None,
        }
    }
}

struct TimeoutConfigInput {
    connect_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    request_timeout: Option<Duration>,
}

fn read_timeout_config<R: ConfigReader + ?Sized>(config: &R) -> ConfigResult<TimeoutConfigInput> {
    Ok(TimeoutConfigInput {
        connect_timeout: config.get_optional("connect_timeout")?,
        read_timeout: config.get_optional("read_timeout")?,
        write_timeout: config.get_optional("write_timeout")?,
        request_timeout: config.get_optional("request_timeout")?,
    })
}

impl TimeoutOptions {
    /// Reads timeout settings from `config` using **relative** keys.
    ///
    /// # Parameters
    /// - `config`: Any [`ConfigReader`] (e.g. root [`qubit_config::Config`] or
    ///   `config.prefix_view("timeouts")`).
    ///
    /// Keys read (all optional; missing keys keep their defaults):
    /// - `connect_timeout`
    /// - `read_timeout`
    /// - `write_timeout`
    /// - `request_timeout`
    ///
    /// # Returns
    /// Populated [`TimeoutOptions`] or [`HttpConfigError`] on type conversion failure.
    pub fn from_config<R: ConfigReader + ?Sized>(config: &R) -> Result<Self, HttpConfigError> {
        let raw = read_timeout_config(config).map_err(HttpConfigError::from)?;

        let mut opts = TimeoutOptions::default();
        if let Some(d) = raw.connect_timeout {
            opts.connect_timeout = d;
        }
        if let Some(d) = raw.read_timeout {
            opts.read_timeout = d;
        }
        if let Some(d) = raw.write_timeout {
            opts.write_timeout = d;
        }
        opts.request_timeout = raw.request_timeout;

        Ok(opts)
    }
}
