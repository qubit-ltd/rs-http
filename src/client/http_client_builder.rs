// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Reqwest-backed HTTP client factory.

use qubit_config::ConfigReader;
use reqwest::Client;
use reqwest::Proxy;
use reqwest::redirect::Policy;
use url::Host;
use url::Url;

use super::RedirectIpv6Violation;
use super::RedirectOriginViolation;
use super::internal::Ipv4OnlyResolver;
use crate::HttpClient;
use crate::HttpClientOptions;
use crate::HttpConfigError;
use crate::HttpError;
use crate::HttpResult;

/// Builder and factory for reqwest-backed [`HttpClient`] instances.
///
/// Options installed with [`HttpClientBuilder::options`] are consumed only by
/// [`HttpClientBuilder::build`]. The `create*` factory methods use their own
/// options argument or defaults and ignore options stored on this builder.
///
/// # Examples
///
/// ```
/// use qubit_http::HttpClientBuilder;
/// use qubit_http::HttpClientOptions;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let options = HttpClientOptions {
///     user_agent: Some("capture-example/1.0".to_owned()),
///     ..HttpClientOptions::default()
/// };
/// let client = HttpClientBuilder::new().options(options).build()?;
/// assert_eq!(client.options().user_agent.as_deref(), Some("capture-example/1.0"));
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Default, Clone)]
pub struct HttpClientBuilder {
    /// Complete client options to apply when building the client.
    options: Option<HttpClientOptions>,
}

impl HttpClientBuilder {
    /// Returns a builder with no stored options.
    ///
    /// # Returns
    /// New [`HttpClientBuilder`].
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self { options: None }
    }

    /// Builds a client from options stored with [`Self::options`], or defaults
    /// when no options were stored.
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
        Self::build_backend(options)
    }

    /// Creates a new [`HttpClient`] with default [`HttpClientOptions`].
    ///
    /// Stored builder options are ignored; this factory uses defaults.
    ///
    /// # Returns
    /// [`HttpClient`] or [`HttpError`] (proxy/build failures).
    ///
    /// # Errors
    /// Returns an error if default options are invalid or the reqwest client
    /// cannot be built.
    #[inline]
    #[deprecated(note = "use HttpClient::builder().build() instead")]
    pub fn create_default(&self) -> HttpResult<HttpClient> {
        Self::build_backend(HttpClientOptions::default())
    }

    /// Applies `options` to a new [`reqwest::Client::builder`], then wraps the
    /// built client.
    ///
    /// Stored builder options are ignored; this factory uses `options`.
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
    #[deprecated(note = "use HttpClient::builder().options(options).build() instead")]
    pub fn create(&self, options: HttpClientOptions) -> HttpResult<HttpClient> {
        Self::build_backend(options)
    }

    /// Builds a reqwest client backend after validating its options.
    ///
    /// # Parameters
    /// - `options`: Complete client configuration.
    ///
    /// # Returns
    /// A configured HTTP client.
    ///
    /// # Errors
    /// Returns a configuration error for invalid options or a client error
    /// when reqwest cannot build the backend.
    fn build_backend(options: HttpClientOptions) -> HttpResult<HttpClient> {
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
        let same_origin_only = matches!(options.origin_policy, crate::HttpOriginPolicy::SameOrigin);
        let ipv4_only = options.ipv4_only;
        builder = builder.redirect(Policy::custom(move |attempt| {
            let same_origin_target = attempt
                .previous()
                .first()
                .is_none_or(|origin| same_origin(origin, attempt.url()));
            if same_origin_only && !same_origin_target {
                return attempt.error(RedirectOriginViolation);
            }
            if ipv4_only && matches!(attempt.url().host(), Some(Host::Ipv6(_))) {
                return attempt.error(RedirectIpv6Violation);
            }
            redirect_policy.redirect(attempt)
        }));
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
            let proxy_url = options
                .proxy
                .to_proxy_url()
                .map_err(map_validation_error)?
                .expect("enabled proxy options produce a proxy URL");
            if options.ipv4_only && matches!(proxy_url.host(), Some(Host::Ipv6(_))) {
                return Err(HttpError::proxy_config(
                    "Proxy host is IPv6, which is not allowed when ipv4_only=true",
                ));
            }
            let mut proxy = Proxy::all(proxy_url)
                .map_err(|error| HttpError::proxy_config(format!("Invalid proxy URL: {error}")))?;

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

    /// Loads [`HttpClientOptions`] from `config` and builds a client.
    ///
    /// Stored builder options are ignored; this factory uses options loaded
    /// from `config`.
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
    ///
    /// # Type Parameters
    /// - `R`: Configuration reader type.
    #[deprecated(note = "use HttpClient::from_config(config) instead")]
    pub fn create_from_config<R>(&self, config: &R) -> Result<HttpClient, HttpConfigError>
    where
        R: ConfigReader + ?Sized,
    {
        HttpClient::from_config(config)
    }

    /// Installs the complete client options value for [`Self::build`].
    ///
    /// The stored options are ignored by the `create*` factory methods.
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
