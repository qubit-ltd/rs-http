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
use qubit_config::Config;

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

    /// Applies `options` to a new [`reqwest::Client::builder`], then wraps the built client.
    ///
    /// # Parameters
    /// - `options`: Full client configuration.
    ///
    /// # Returns
    /// [`HttpClient`] or [`HttpError`] (proxy/build failures).
    pub fn create(&self, options: HttpClientOptions) -> HttpResult<HttpClient> {
        let mut builder = reqwest::Client::builder();

        builder = builder.connect_timeout(options.timeouts.connect_timeout);
        if let Some(request_timeout) = options.timeouts.request_timeout {
            builder = builder.timeout(request_timeout);
        }

        if !options.default_headers.is_empty() {
            builder = builder.default_headers(options.default_headers.clone());
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

        let client = builder.build().map_err(|error| {
            HttpError::build_client(format!("Failed to build reqwest client: {}", error))
                .with_source(error)
        })?;

        Ok(HttpClient::new(client, options))
    }

    /// Loads [`HttpClientOptions`] from `config`, validates them, then calls [`HttpClientFactory::create`].
    ///
    /// # Parameters
    /// - `config`: Application configuration.
    /// - `prefix`: Logical key prefix; options are read via [`qubit_config::Config::prefix_view`].
    ///
    /// # Returns
    /// - `Ok(HttpClient)` when parsing, validation, and client build succeed.
    /// - `Err(HttpConfigError)` on config or validation errors; build failures are mapped to [`HttpConfigError`].
    pub fn create_from_config(
        &self,
        config: &Config,
        prefix: &str,
    ) -> Result<HttpClient, HttpConfigError> {
        let view = config.prefix_view(prefix);
        let options = HttpClientOptions::from_config(&view)
            .map_err(|e| prefix_config_error(prefix, e))?;
        options
            .validate()
            .map_err(|e| prefix_config_error(prefix, e))?;
        self.create(options).map_err(|e| {
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
    let path = if error.path.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}.{}", error.path)
    };
    HttpConfigError::new(error.kind, path, error.message)
}
