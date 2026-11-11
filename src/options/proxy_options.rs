// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt;
use std::net::Ipv6Addr;
use std::str::FromStr;

use qubit_argument::require_that;
use qubit_argument::ArgumentResult;
use qubit_config::ConfigReader;
use qubit_config::ConfigResult;
use url::Host;
use url::Url;

use super::internal::ProxyConfigInput;
use super::proxy_type::ProxyType;
use super::HttpConfigError;

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
///
/// # Type Parameters
/// - `R`: Configuration reader type.
fn read_proxy_config<R>(config: &R) -> ConfigResult<ProxyConfigInput>
where
    R: ConfigReader + ?Sized,
{
    super::from_config_helpers::ensure_known_config_keys(
        config,
        &[
            "enabled",
            "proxy_type",
            "host",
            "port",
            "username",
            "password",
        ],
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
    ///
    /// # Type Parameters
    /// - `R`: Configuration reader type.
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

        opts.validate().map_err(|error| {
            super::from_config_helpers::resolve_component_error(config, error, "proxy")
        })?;
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

    /// Builds a proxy URL from the validated host and independent port.
    ///
    /// # Returns
    /// `None` when the proxy is disabled, or a URL containing the configured
    /// scheme, pure host, and port when enabled.
    ///
    /// # Errors
    /// Returns [`HttpConfigError`] when enabled options are invalid or the
    /// host or port cannot be represented in a URL.
    pub(crate) fn to_proxy_url(&self) -> Result<Option<Url>, HttpConfigError> {
        self.validate()?;
        if !self.enabled {
            return Ok(None);
        }
        let host = parse_proxy_host(self.host.as_deref().expect("validated proxy host exists"))
            .map_err(|_| HttpConfigError::invalid_value("proxy.host", "Invalid proxy host"))?;
        let port = self.port.expect("validated proxy port exists");
        let mut url = Url::parse(&format!("{}://proxy.invalid/", self.proxy_type.scheme()))
            .expect("supported proxy scheme with a fixed authority is valid");
        match host {
            Host::Domain(domain) => url
                .set_host(Some(&domain))
                .map_err(|_| HttpConfigError::invalid_value("proxy.host", "Invalid proxy host"))?,
            Host::Ipv4(ip) => url
                .set_host(Some(&ip.to_string()))
                .map_err(|_| HttpConfigError::invalid_value("proxy.host", "Invalid proxy host"))?,
            Host::Ipv6(ip) => url
                .set_host(Some(&format!("[{ip}]")))
                .map_err(|_| HttpConfigError::invalid_value("proxy.host", "Invalid proxy host"))?,
        }
        url.set_port(Some(port))
            .map_err(|_| HttpConfigError::invalid_value("proxy.port", "Invalid proxy port"))?;
        Ok(Some(url))
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
                |host| host.is_none_or(|value| parse_proxy_host(value).is_ok()),
                "invalid_proxy_host",
                "Proxy host must be a pure domain name or IP address",
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

/// Parses a pure proxy domain name or IP address without trimming the input.
///
/// # Parameters
/// - `value`: Host text without a scheme, credentials, path, query, fragment,
///   or port. A valid IPv6 address may be bracketed or unbracketed.
///
/// # Returns
/// The parsed [`Host<String>`] domain, IPv4 address, or IPv6 address. This
/// function does not check DNS reachability.
///
/// # Errors
/// Returns `()` for empty or whitespace-containing input, forbidden authority
/// separators, invalid brackets, a port, or a host rejected by the URL host
/// parser.
fn parse_proxy_host(value: &str) -> Result<Host<String>, ()> {
    if value.is_empty()
        || value.trim() != value
        || value.chars().any(char::is_whitespace)
        || value
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\' | b'@' | b'?' | b'#' | b'%'))
    {
        return Err(());
    }
    let ipv6 = if value.starts_with('[') && value.ends_with(']') {
        &value[1..value.len() - 1]
    } else {
        value
    };
    if let Ok(address) = ipv6.parse::<Ipv6Addr>() {
        return Ok(Host::Ipv6(address));
    }
    if value.starts_with('[') || value.ends_with(']') || value.contains(':') {
        return Err(());
    }
    Host::parse(value).map_err(|_| ())
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
            format!(
                "Unknown proxy type '{}'; expected http, https, or socks5",
                s,
            ),
        )
    })
}
