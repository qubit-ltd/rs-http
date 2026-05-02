/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Reqwest-backed HTTP client factory.

use std::net::{IpAddr, SocketAddr};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::Policy;

use crate::HttpConfigError;
use crate::{HttpClient, HttpClientOptions, HttpError, HttpResult};
use qubit_config::ConfigReader;
use qubit_error::{BoxError, IntoBoxError};

/// DNS resolver that filters out non-IPv4 addresses for `ipv4_only` mode.
#[derive(Debug, Clone, Copy, Default)]
struct Ipv4OnlyResolver;

impl Resolve for Ipv4OnlyResolver {
    /// Resolves `name` using the OS resolver and returns only IPv4 addresses.
    ///
    /// # Parameters
    /// - `name`: DNS name to resolve.
    ///
    /// # Returns
    /// A future yielding only IPv4 socket addresses.
    ///
    /// # Errors
    /// Returns an error when DNS lookup fails or when no IPv4 address exists for `name`.
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let resolved = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|error| error.into_box_error())?;
            filter_ipv4_addrs(&host, resolved)
        })
    }
}

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
    pub fn create_default(&self) -> HttpResult<HttpClient> {
        self.create(HttpClientOptions::default())
    }

    /// Applies `options` to a new [`reqwest::Client::builder`], then wraps the built client.
    ///
    /// # Parameters
    /// - `options`: Full client configuration.
    ///
    /// # Returns
    /// [`HttpClient`] or [`HttpError`] (proxy/build failures).
    pub fn create(&self, options: HttpClientOptions) -> HttpResult<HttpClient> {
        options.validate().map_err(map_validation_error)?;

        let mut builder = reqwest::Client::builder();

        builder = builder.connect_timeout(options.timeouts.connect_timeout);
        if let Some(request_timeout) = options.timeouts.request_timeout {
            builder = builder.timeout(request_timeout);
        }
        if let Some(user_agent) = options.user_agent.as_deref() {
            builder = builder.user_agent(user_agent);
        }
        if let Some(max_redirects) = options.max_redirects {
            builder = builder.redirect(Policy::limited(max_redirects));
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
            let mut proxy = reqwest::Proxy::all(&proxy_url).map_err(|error| {
                HttpError::proxy_config(format!("Invalid proxy URL '{}': {}", proxy_url, error))
            })?;

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
    /// [`HttpClientFactory::create`].
    ///
    /// # Parameters
    /// - `config`: Any [`ConfigReader`] (root [`qubit_config::Config`] or a
    ///   [`qubit_config::ConfigPrefixView`] from [`ConfigReader::prefix_view`]).
    ///
    /// # Returns
    /// - `Ok(HttpClient)` when parsing, validation, and client build succeed.
    /// - `Err(HttpConfigError)` on config or validation errors; build failures are mapped to [`HttpConfigError`].
    pub fn create_from_config<R>(&self, config: &R) -> Result<HttpClient, HttpConfigError>
    where
        R: ConfigReader + ?Sized,
    {
        let options =
            HttpClientOptions::from_config(config).map_err(|e| resolve_config_error(config, e))?;
        options
            .validate()
            .map_err(|e| resolve_config_error(config, e))?;
        self.create(options).map_err(|e| {
            HttpConfigError::new(
                crate::HttpConfigErrorKind::InvalidValue,
                config.resolve_key(""),
                e.to_string(),
            )
        })
    }
}

fn resolve_config_error<R>(config: &R, mut error: HttpConfigError) -> HttpConfigError
where
    R: ConfigReader + ?Sized,
{
    error.path = if error.path.is_empty() {
        config.resolve_key("")
    } else {
        config.resolve_key(&error.path)
    };
    error
}

/// Filters resolved socket addresses down to IPv4 addresses.
///
/// # Parameters
/// - `host`: Hostname used for diagnostics when no IPv4 address remains.
/// - `resolved`: Iterator of resolved socket addresses.
///
/// # Returns
/// Boxed reqwest DNS address iterator containing only IPv4 addresses.
///
/// # Errors
/// Returns an [`std::io::ErrorKind::AddrNotAvailable`] error when resolution
/// produced no IPv4 address.
fn filter_ipv4_addrs<I>(host: &str, resolved: I) -> Result<Addrs, BoxError>
where
    I: IntoIterator<Item = SocketAddr>,
{
    let ipv4_addrs: Vec<SocketAddr> = resolved.into_iter().filter(SocketAddr::is_ipv4).collect();
    if ipv4_addrs.is_empty() {
        let error = std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            format!("No IPv4 address found for host '{host}'"),
        );
        return Err(error.into_box_error());
    }
    Ok(Box::new(ipv4_addrs.into_iter()) as Addrs)
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

/// Exercises factory-only defensive helpers for coverage tests.
///
/// # Returns
/// Diagnostic values proving IPv4 filtering, config path resolution, validation
/// mapping, and IPv6 literal parsing paths were executed.
#[cfg(coverage)]
#[doc(hidden)]
pub(crate) fn coverage_exercise_factory_paths() -> Vec<String> {
    let no_ipv4_error = filter_ipv4_addrs(
        "coverage-host",
        [SocketAddr::new(
            IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
            0,
        )],
    )
    .err()
    .expect("IPv6-only addresses should be rejected")
    .to_string();
    let ipv4_count = filter_ipv4_addrs(
        "coverage-host",
        [SocketAddr::new(IpAddr::from([127, 0, 0, 1]), 0)],
    )
    .expect("IPv4 address should be accepted")
    .count()
    .to_string();
    let config = qubit_config::Config::new();
    let scoped_path = resolve_config_error(
        &config.prefix_view("coverage"),
        HttpConfigError::invalid_value("", "coverage error"),
    )
    .path;
    let proxy_kind = map_validation_error(HttpConfigError::invalid_value("proxy.host", "bad")).kind;
    let other_kind = map_validation_error(HttpConfigError::invalid_value("user_agent", "bad")).kind;

    vec![
        no_ipv4_error,
        ipv4_count,
        scoped_path,
        format!("{proxy_kind:?}"),
        format!("{other_kind:?}"),
        is_ipv6_literal_host("[::1]").to_string(),
        is_ipv6_literal_host("127.0.0.1").to_string(),
    ]
}
