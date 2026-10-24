// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Reqwest-backed HTTP client factory.

use std::net::IpAddr;

use qubit_config::ConfigReader;
use reqwest::Client;
use reqwest::Proxy;
use reqwest::redirect::Policy;
use url::Url;

use super::RedirectOriginViolation;
use super::internal::Ipv4OnlyResolver;
use crate::HttpClient;
use crate::HttpClientOptions;
use crate::HttpConfigError;
use crate::HttpError;
use crate::HttpResult;

/// Public factory used to build reqwest-backed [`HttpClient`] instances.
///
/// # Examples
///
/// ```
/// use qubit_http::HttpClientBuilder;
/// use qubit_http::HttpClientOptions;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = HttpClientBuilder::new()
///     .options(HttpClientOptions::new())
///     .build()?;
/// assert!(!client.options().retry.enabled);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Default, Clone)]
pub struct HttpClientBuilder {
    /// Complete client options to apply when building the client.
    options: Option<HttpClientOptions>,
}

impl HttpClientBuilder {
    /// Returns a stateless factory instance.
    ///
    /// # Returns
    /// New [`HttpClientBuilder`].
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self { options: None }
    }

    /// Builds a client from the configured options or defaults.
    ///
    /// # Returns
    /// A client using the configured options, or the default options when none
    /// were installed.
    ///
    /// # Errors
    /// Returns an error when options are invalid or the reqwest client cannot
    /// be built.
    pub fn build(self) -> HttpResult<HttpClient> {
        let options = self.options.unwrap_or_default();
        Self::new().create(options)
    }

    /// Creates a new [`HttpClient`] with default [`HttpClientOptions`].
    ///
    /// # Returns
    /// [`HttpClient`] or [`HttpError`] (proxy/build failures).
    ///
    /// # Errors
    /// Returns an error if default options are invalid or the reqwest client
    /// cannot be built.
    #[inline]
    pub fn create_default(&self) -> HttpResult<HttpClient> {
        self.create(HttpClientOptions::default())
    }

    /// Applies `options` to a new [`reqwest::Client::builder`], then wraps the
    /// built client.
    ///
    /// # Parameters
    /// - `options`: Full client configuration.
    ///
    /// # Returns
    /// [`HttpClient`] or [`HttpError`] (proxy/build failures).
    ///
    /// # Errors
    /// Returns an error if `options` are invalid or reqwest cannot build the
    /// configured client.
    pub fn create(&self, options: HttpClientOptions) -> HttpResult<HttpClient> {
        options.validate().map_err(map_validation_error)?;

        let mut builder = Client::builder();

        builder = builder.connect_timeout(options.timeouts.connect_timeout);
        if let Some(request_timeout) = options.timeouts.request_timeout {
            builder = builder.timeout(request_timeout);
        }
        if let Some(user_agent) = options.user_agent.as_deref() {
            builder = builder.user_agent(user_agent);
        }
        let redirect_policy = options.max_redirects.map(Policy::limited).unwrap_or_default();
        if matches!(options.origin_policy, crate::HttpOriginPolicy::SameOrigin) {
            let redirect_policy = Policy::custom(move |attempt| {
                let same_origin = attempt
                    .previous()
                    .first()
                    .is_none_or(|origin| same_origin(origin, attempt.url()));
                if same_origin {
                    redirect_policy.redirect(attempt)
                } else {
                    attempt.error(RedirectOriginViolation)
                }
            });
            builder = builder.redirect(redirect_policy);
        } else {
            builder = builder.redirect(redirect_policy);
        }
        if let Some(pool_idle_timeout) = options.pool_idle_timeout {
            builder = builder.pool_idle_timeout(pool_idle_timeout);
        }
        if let Some(pool_max_idle_per_host) = options.pool_max_idle_per_host {
            builder = builder.pool_max_idle_per_host(pool_max_idle_per_host);
        }
        if options.ipv4_only {
            builder = builder.dns_resolver(Ipv4OnlyResolver);
        }

        if options.proxy.enabled {
            let host = options
                .proxy
                .host
                .clone()
                .expect("proxy.host must exist after HttpClientOptions::validate");
            if options.ipv4_only && is_ipv6_literal_host(&host) {
                return Err(HttpError::proxy_config(format!(
                    "Proxy host '{host}' is IPv6, which is not allowed when ipv4_only=true",
                )));
            }
            let port = options
                .proxy
                .port
                .expect("proxy.port must exist after HttpClientOptions::validate");

            let proxy_url = format!("{}://{}:{}", options.proxy.proxy_type.scheme(), host, port);
            let mut proxy = Proxy::all(&proxy_url)
                .map_err(|error| HttpError::proxy_config(format!("Invalid proxy URL '{}': {}", proxy_url, error)))?;

            if let Some(username) = options.proxy.username.clone() {
                let password = options.proxy.password.as_deref().unwrap_or("");
                proxy = proxy.basic_auth(&username, password);
            }

            builder = builder.proxy(proxy);
        } else if !options.use_env_proxy {
            // Keep behavior aligned with explicit proxy switch semantics:
            // when both explicit proxy and env proxy inheritance are disabled,
            // do not inherit environment proxies.
            builder = builder.no_proxy();
        }

        let backend = builder.build().map_err(HttpError::from)?;

        Ok(HttpClient::new(backend, options))
    }

    /// Loads [`HttpClientOptions`] from `config`, validates them, then calls
    /// [`HttpClientBuilder::create`].
    ///
    /// # Parameters
    /// - `config`: Any [`ConfigReader`] (root [`qubit_config::Config`] or a
    ///   [`qubit_config::ConfigSection`] from [`ConfigReader::section`]).
    ///
    /// # Returns
    /// - `Ok(HttpClient)` when parsing, validation, and client build succeed.
    /// - `Err(HttpConfigError)` on config or validation errors; build failures
    ///   are mapped to [`HttpConfigError`].
    ///
    /// # Errors
    /// Returns an error when reading or validating `config` fails, or when the
    /// configured reqwest client cannot be built.
    pub fn create_from_config<R>(&self, config: &R) -> Result<HttpClient, HttpConfigError>
    where
        R: ConfigReader + ?Sized,
    {
        let options = HttpClientOptions::from_config(config)?;
        self.create(options).map_err(|e| {
            HttpConfigError::new(
                crate::HttpConfigErrorKind::InvalidValue,
                config.scope_path(),
                e.to_string(),
            )
        })
    }

    /// Installs the complete client options value.
    ///
    /// # Parameters
    /// - `options`: Complete options to use when building the client.
    ///
    /// # Returns
    /// A builder that owns `options` and can build a client.
    #[must_use]
    #[inline]
    pub fn options(mut self, options: HttpClientOptions) -> Self {
        self.options = Some(options);
        self
    }
}

/// Maps options validation errors to runtime [`HttpError`] values.
///
/// # Parameters
/// - `error`: Validation error returned by [`HttpClientOptions::validate`].
///
/// # Returns
/// - [`HttpError::proxy_config`] for proxy section validation failures;
/// - [`HttpError::other`] for all other option validation failures.
fn map_validation_error(error: HttpConfigError) -> HttpError {
    if error.path.starts_with("proxy.") {
        HttpError::proxy_config(error.to_string())
    } else {
        HttpError::other(format!("Invalid HTTP client options: {error}"))
    }
}

/// Checks whether `host` is an IPv6 literal value.
///
/// # Parameters
/// - `host`: Raw host text, optionally wrapped by square brackets.
///
/// # Returns
/// `true` if `host` parses as an IPv6 literal.
fn is_ipv6_literal_host(host: &str) -> bool {
    let trimmed = host.trim().trim_start_matches('[').trim_end_matches(']');
    matches!(trimmed.parse::<IpAddr>(), Ok(IpAddr::V6(_)))
}

/// Returns whether two URLs share scheme, host, and effective port.
///
/// # Parameters
/// - `left`: First URL to compare.
/// - `right`: Second URL to compare.
///
/// # Returns
/// `true` when both URLs have the same scheme, host, and effective port.
fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host() == right.host()
        && left.port_or_known_default() == right.port_or_known_default()
}
