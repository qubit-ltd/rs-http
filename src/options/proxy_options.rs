/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::fmt;
use std::str::FromStr;

use qubit_config::{
    ConfigReader,
    ConfigResult,
};

use super::HttpConfigError;

use super::proxy_type::ProxyType;

/// Outbound proxy configuration applied when building the reqwest client.
#[derive(Clone, PartialEq, Eq)]
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

impl fmt::Debug for ProxyOptions {
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

fn read_proxy_config<R>(config: &R) -> ConfigResult<ProxyConfigInput>
where
    R: ConfigReader + ?Sized,
{
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
            match self.host.as_deref() {
                None => {
                    return Err(HttpConfigError::missing(
                        "proxy.host",
                        "Proxy is enabled but host is missing",
                    ));
                }
                Some(host) if host.trim().is_empty() => {
                    return Err(HttpConfigError::invalid_value(
                        "proxy.host",
                        "Proxy host cannot be empty when proxy is enabled",
                    ));
                }
                _ => {}
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
        if let Some(username) = self.username.as_deref() {
            if username.trim().is_empty() {
                return Err(HttpConfigError::invalid_value(
                    "proxy.username",
                    "Proxy username cannot be empty when provided",
                ));
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
