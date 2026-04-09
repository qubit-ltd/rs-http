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

use super::proxy_type::ProxyType;

/// Outbound proxy configuration applied when building the reqwest client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyOptions {
    /// Whether proxy is enabled.
    pub enabled: bool,
    /// Proxy type.
    pub proxy_type: ProxyType,
    /// Proxy host.
    pub host: Option<String>,
    /// Proxy port.
    pub port: Option<u16>,
    /// Proxy username.
    pub username: Option<String>,
    /// Proxy password.
    pub password: Option<String>,
}

impl Default for ProxyOptions {
    /// Proxy disabled; type HTTP; no host, port, or credentials.
    ///
    /// # Returns
    /// Default [`ProxyOptions`].
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

struct ProxyConfigInput {
    enabled: Option<bool>,
    proxy_type: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    username: Option<String>,
    password: Option<String>,
}

fn read_proxy_config<R: ConfigReader + ?Sized>(config: &R) -> ConfigResult<ProxyConfigInput> {
    Ok(ProxyConfigInput {
        enabled: config.get_optional("enabled")?,
        proxy_type: config.get_optional_string("proxy_type")?,
        host: config.get_optional_string("host")?,
        port: config.get_optional("port")?,
        username: config.get_optional_string("username")?,
        password: config.get_optional_string("password")?,
    })
}

impl ProxyOptions {
    /// Reads proxy settings from `config` using **relative** keys.
    ///
    /// # Parameters
    /// - `config`: Any [`ConfigReader`] (e.g. `config.prefix_view("proxy")`).
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
    /// Populated [`ProxyOptions`] or [`HttpConfigError`].
    pub fn from_config<R: ConfigReader + ?Sized>(config: &R) -> Result<Self, HttpConfigError> {
        let raw = read_proxy_config(config).map_err(HttpConfigError::from)?;

        let mut opts = ProxyOptions::default();
        if let Some(v) = raw.enabled {
            opts.enabled = v;
        }
        if let Some(s) = raw.proxy_type {
            opts.proxy_type = parse_proxy_type("proxy_type", &s)?;
        }
        opts.host = raw.host;
        if let Some(p) = raw.port {
            opts.port = Some(p);
        }
        opts.username = raw.username;
        opts.password = raw.password;

        Ok(opts)
    }

    /// Validates proxy options for internal consistency.
    ///
    /// # Returns
    /// - `Ok(())` when disabled or when enabled with valid host/port and credential pairing.
    /// - `Err(HttpConfigError)` if proxy is enabled but host/port invalid, or password without username.
    pub fn validate(&self) -> Result<(), HttpConfigError> {
        if self.enabled {
            if self.host.is_none() {
                return Err(HttpConfigError::missing(
                    "proxy.host",
                    "Proxy is enabled but host is missing",
                ));
            }
            match self.port {
                None => {
                    return Err(HttpConfigError::missing(
                        "proxy.port",
                        "Proxy is enabled but port is missing",
                    ));
                }
                Some(0) => {
                    return Err(HttpConfigError::invalid_value(
                        "proxy.port",
                        "Proxy port must be greater than 0",
                    ));
                }
                _ => {}
            }
        }
        if self.username.is_none() && self.password.is_some() {
            return Err(HttpConfigError::missing(
                "proxy.username",
                "Proxy password is configured but username is missing",
            ));
        }
        Ok(())
    }
}

fn parse_proxy_type(path: &str, s: &str) -> Result<ProxyType, HttpConfigError> {
    match s.to_lowercase().as_str() {
        "http" => Ok(ProxyType::Http),
        "https" => Ok(ProxyType::Https),
        "socks5" | "socks5h" => Ok(ProxyType::Socks5),
        other => Err(HttpConfigError::invalid_value(
            path,
            format!(
                "Unknown proxy type '{}'; expected http, https, or socks5",
                other
            ),
        )),
    }
}
