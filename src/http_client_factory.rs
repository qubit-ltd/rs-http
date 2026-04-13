/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Reqwest-backed HTTP client factory.

use crate::HttpConfigError;
use crate::{HttpClient, HttpClientOptions, HttpError, HttpResult};
use qubit_config::ConfigReader;

/// Public factory used to build reqwest-backed [`HttpClient`] instances.
#[derive(Debug, Default, Clone, Copy)]
pub struct HttpClientFactory;

impl HttpClientFactory {
    /// Returns a stateless factory instance.
    ///
    /// # Returns
    /// New [`HttpClientFactory`].
    pub fn new() -> Self {
        Self
    }

    /// Creates a new [`HttpClient`] with default [`HttpClientOptions`].
    ///
    /// # Returns
    /// [`HttpClient`] or [`HttpError`] (proxy/build failures).
    pub fn create(&self) -> HttpResult<HttpClient> {
        self.create_with_options(HttpClientOptions::default())
    }

    /// Applies `options` to a new [`reqwest::Client::builder`], then wraps the built client.
    ///
    /// # Parameters
    /// - `options`: Full client configuration.
    ///
    /// # Returns
    /// [`HttpClient`] or [`HttpError`] (proxy/build failures).
    pub fn create_with_options(&self, options: HttpClientOptions) -> HttpResult<HttpClient> {
        let mut builder = reqwest::Client::builder();

        builder = builder.connect_timeout(options.timeouts.connect_timeout);
        if let Some(request_timeout) = options.timeouts.request_timeout {
            builder = builder.timeout(request_timeout);
        }

        if options.proxy.enabled {
            let host =
                options.proxy.host.clone().ok_or_else(|| {
                    HttpError::proxy_config("Proxy is enabled but host is missing")
                })?;
            let port = options
                .proxy
                .port
                .ok_or_else(|| HttpError::proxy_config("Proxy is enabled but port is missing"))?;
            if port == 0 {
                return Err(HttpError::proxy_config("Proxy port must be greater than 0"));
            }

            let proxy_url = format!("{}://{}:{}", options.proxy.proxy_type.scheme(), host, port);
            let mut proxy = reqwest::Proxy::all(&proxy_url).map_err(|error| {
                HttpError::proxy_config(format!("Invalid proxy URL '{}': {}", proxy_url, error))
            })?;

            if options.proxy.username.is_none() && options.proxy.password.is_some() {
                return Err(HttpError::proxy_config(
                    "Proxy password is configured but username is missing",
                ));
            }

            if let Some(username) = options.proxy.username.clone() {
                let password = options.proxy.password.as_deref().unwrap_or("");
                proxy = proxy.basic_auth(&username, password);
            }

            builder = builder.proxy(proxy);
        } else {
            // Keep behavior aligned with explicit proxy switch semantics:
            // when proxy is disabled, do not inherit environment proxies.
            builder = builder.no_proxy();
        }

        if options.ipv4_only {
            tracing::warn!(
                "IPv4-only mode is requested but not yet enforced at transport resolver level"
            );
        }

        let client = builder.build().map_err(HttpError::from)?;

        Ok(HttpClient::new(client, options))
    }

    /// Loads [`HttpClientOptions`] from `config`, validates them, then calls
    /// [`HttpClientFactory::create_with_options`].
    ///
    /// # Parameters
    /// - `config`: Any [`ConfigReader`] (root [`qubit_config::Config`] or a
    ///   [`qubit_config::ConfigPrefixView`] from [`ConfigReader::prefix_view`]).
    /// - `prefix`: Logical key prefix relative to `config`; options are read via
    ///   [`ConfigReader::prefix_view`] (empty or dot-only values resolve to the root).
    ///
    /// # Returns
    /// - `Ok(HttpClient)` when parsing, validation, and client build succeed.
    /// - `Err(HttpConfigError)` on config or validation errors; build failures are mapped to [`HttpConfigError`].
    pub fn create_from_config<R>(
        &self,
        config: &R,
        prefix: &str,
    ) -> Result<HttpClient, HttpConfigError>
    where
        R: ConfigReader + ?Sized,
    {
        let view = config.prefix_view(prefix);
        let options =
            HttpClientOptions::from_config(&view).map_err(|e| prefix_config_error(prefix, e))?;
        options
            .validate()
            .map_err(|e| prefix_config_error(prefix, e))?;
        self.create_with_options(options).map_err(|e| {
            HttpConfigError::new(
                crate::HttpConfigErrorKind::InvalidValue,
                prefix,
                e.to_string(),
            )
        })
    }
}

fn prefix_config_error(prefix: &str, error: HttpConfigError) -> HttpConfigError {
    if prefix.is_empty() {
        return error;
    }
    let prefix_with_dot = format!("{prefix}.");
    let already_prefixed = error.path == prefix || error.path.starts_with(&prefix_with_dot);
    let path = if already_prefixed {
        error.path
    } else {
        error
            .path
            .find(&prefix_with_dot)
            .map(|index| error.path[index..].to_string())
            .unwrap_or_else(|| {
                [prefix, error.path.as_str()]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(".")
            })
    };
    HttpConfigError::new(error.kind, path, error.message)
}
