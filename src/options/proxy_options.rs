// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt;
use std::str::FromStr;

use qubit_argument::ArgumentResult;
use qubit_argument::require_that;
use qubit_config::ConfigReader;
use qubit_config::ConfigResult;

use super::HttpConfigError;
use super::internal::ProxyConfigInput;
use super::proxy_type::ProxyType;

/// Outbound proxy settings used when building the reqwest client.
///
/// The options retain credentials as configuration data; the custom `Debug`
/// implementation masks the password. Call [`validate`](Self::validate) to
/// check enabled-proxy requirements before using the settings.
///
/// # Examples
///
/// ```
/// use qubit_http::{ProxyOptions, ProxyType};
///
/// let mut proxy = ProxyOptions::default();
/// proxy.enabled = true;
/// proxy.proxy_type = ProxyType::Http;
/// proxy.host = Some("proxy.example".to_owned());
/// proxy.port = Some(8080);
/// assert!(proxy.validate().is_ok());
/// ```
#[must_use]
#[derive(Clone, PartialEq, Eq)]
pub struct ProxyOptions {
    /// Whether requests should use this proxy configuration.
    pub enabled: bool,
    /// URL scheme used to connect to the proxy.
    pub proxy_type: ProxyType,
    /// Proxy host name or address; required and nonblank when enabled.
    pub host: Option<String>,
    /// Proxy port; required and nonzero when enabled.
    pub port: Option<u16>,
    /// Optional authentication username; blank values are rejected.
    pub username: Option<String>,
    /// Optional authentication password, which requires a username.
    pub password: Option<String>,
}

impl fmt::Debug for ProxyOptions {
    /// Formats the settings while masking any configured password.
    ///
    /// # Parameters
    /// - `formatter`: Destination receiving the debug representation.
    ///
    /// # Returns
    /// The formatter's result, including any formatting error.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let password = self.password.as_ref().map(|_| "****");
        formatter
            .debug_struct("ProxyOptions")
            .field("enabled", &self.enabled)
            .field("proxy_type", &self.proxy_type)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &password)
            .finish()
    }
}

impl Default for ProxyOptions {
    /// Proxy disabled; type HTTP; no host, port, or credentials.
    ///
    /// # Returns
    /// A disabled [`ProxyOptions`] with HTTP type and no host, port, or
    /// credentials.
    #[inline]
    fn default() -> Self {
        Self {
            enabled: false,
            proxy_type: ProxyType::Http,
            host: None,
            port: None,
            username: None,
            password: None,
        }
    }
}

/// Reads recognized relative proxy keys without applying defaults or
/// semantic validation.
///
/// # Parameters
/// - `config`: Source queried for the relative proxy keys.
///
/// # Returns
/// The values that were present, with absent optional keys represented as
/// `None`.
///
/// # Errors
/// Returns the configuration reader's error when a key is unknown or cannot
/// be read or converted to its expected type.
fn read_proxy_config<R>(config: &R) -> ConfigResult<ProxyConfigInput>
where
    R: ConfigReader + ?Sized,
{
    super::from_config_helpers::ensure_known_config_keys(
        config,
        &["enabled", "proxy_type", "host", "port", "username", "password"],
        &[],
    )?;
    Ok(ProxyConfigInput {
        enabled: config.get_optional("enabled")?,
        proxy_type: config.get_optional_interpolated::<String>("proxy_type")?,
        host: config.get_optional_interpolated::<String>("host")?,
        port: config.get_optional("port")?,
        username: config.get_optional_interpolated::<String>("username")?,
        password: config.get_optional_interpolated::<String>("password")?,
    })
}

impl ProxyOptions {
    /// Reads proxy settings from `config` using **relative** keys.
    ///
    /// # Parameters
    /// - `config`: Any [`ConfigReader`] (e.g. `config.section("proxy")`).
    ///
    /// Keys read:
    /// - `enabled`
    /// - `proxy_type`
    /// - `host`
    /// - `port`
    /// - `username`
    /// - `password`
    ///
    /// # Returns
    /// The resulting settings, with defaults applied to omitted keys.
    ///
    /// # Errors
    /// Returns [`HttpConfigError`] when reading or converting a key fails, the
    /// proxy type is unknown, or the resulting enabled settings are invalid.
    pub fn from_config<R>(config: &R) -> Result<Self, HttpConfigError>
    where
        R: ConfigReader + ?Sized,
    {
        let raw = read_proxy_config(config).map_err(HttpConfigError::from)?;

        let mut opts = ProxyOptions::default();
        if let Some(v) = raw.enabled {
            opts.enabled = v;
        }
        if let Some(s) = raw.proxy_type {
            opts.proxy_type = parse_proxy_type("proxy_type", &s)
                .map_err(|error| super::from_config_helpers::resolve_config_error(config, error))?;
        }
        opts.host = raw.host;
        if let Some(p) = raw.port {
            opts.port = Some(p);
        }
        opts.username = raw.username;
        opts.password = raw.password;

        opts.validate()
            .map_err(|error| super::from_config_helpers::resolve_component_error(config, error, "proxy"))?;
        Ok(opts)
    }

    /// Validates proxy options for internal consistency.
    ///
    /// # Returns
    /// - `Ok(())` when disabled or when enabled with valid host/port and
    ///   credential pairing.
    /// - `Err(HttpConfigError)` if proxy is enabled but host/port invalid, or
    ///   password without username.
    ///
    /// # Errors
    /// Returns [`HttpConfigError`] with the offending field path when enabled
    /// proxy requirements or credential pairing are invalid.
    #[inline]
    pub fn validate(&self) -> Result<(), HttpConfigError> {
        self.validate_arguments().map_err(HttpConfigError::from)
    }

    /// Validates proxy fields while retaining structured argument errors.
    ///
    /// # Parameters
    /// - `self`: Proxy settings whose fields are checked.
    ///
    /// # Returns
    /// `Ok(())` when the fields satisfy the proxy requirements.
    ///
    /// # Errors
    /// Returns a structured argument error identifying the first invalid
    /// field and its validation condition.
    pub(super) fn validate_arguments(&self) -> ArgumentResult<()> {
        if self.enabled {
            require_that(
                self.host.as_deref(),
                "proxy.host",
                Option::is_some,
                "http_config_missing",
                "Proxy is enabled but host is missing",
            )?;
            require_that(
                self.host.as_deref(),
                "proxy.host",
                |host| host.is_none_or(|value| !value.trim().is_empty()),
                "blank_proxy_host",
                "Proxy host cannot be empty when proxy is enabled",
            )?;
            require_that(
                self.port,
                "proxy.port",
                Option::is_some,
                "http_config_missing",
                "Proxy is enabled but port is missing",
            )?;
            require_that(
                self.port,
                "proxy.port",
                |port| port.is_none_or(|value| value > 0),
                "zero_proxy_port",
                "Proxy port must be greater than 0",
            )?;
        }
        if let Some(username) = self.username.as_deref() {
            require_that(
                username,
                "proxy.username",
                |value| !value.trim().is_empty(),
                "blank_proxy_username",
                "Proxy username cannot be empty when provided",
            )?;
        }
        if self.password.is_some() {
            require_that(
                self.username.as_deref(),
                "proxy.username",
                Option::is_some,
                "http_config_missing",
                "Proxy password is configured but username is missing",
            )?;
        }
        Ok(())
    }
}

/// Parses a proxy scheme after trimming surrounding whitespace.
///
/// # Parameters
/// - `path`: Configuration key associated with the value, used in errors.
/// - `s`: Configured proxy type spelling.
///
/// # Returns
/// The corresponding [`ProxyType`].
///
/// # Errors
/// Returns [`HttpConfigError`] when the value is not a supported proxy type.
fn parse_proxy_type(path: &str, s: &str) -> Result<ProxyType, HttpConfigError> {
    ProxyType::from_str(s.trim()).map_err(|_| {
        HttpConfigError::invalid_value(
            path,
            format!("Unknown proxy type '{}'; expected http, https, or socks5", s,),
        )
    })
}
