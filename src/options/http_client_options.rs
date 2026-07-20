// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use http::HeaderMap;
use http::HeaderValue;
use qubit_argument::{
    require_that,
    ArgumentResultExt,
};
use qubit_config::{
    ConfigReader,
    ConfigResult,
};
use qubit_redact::{
    http::UrlPathPolicy,
    RedactionPolicy,
    Sensitivity,
};
use std::str::FromStr;
use url::Url;

use super::from_config_helpers::{
    get_optional_usize,
    hashmap_to_headermap,
};
use super::http_logging_options::HttpLoggingOptions;
use super::http_retry_options::HttpRetryOptions;
use super::http_timeout_options::HttpTimeoutOptions;
use super::internal::HttpClientLogRedactionConfigInput;
use super::proxy_options::ProxyOptions;
use super::HttpConfigError;
use crate::{
    constants::{
        DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES,
        DEFAULT_RESPONSE_BODY_SIZE_LIMIT_BYTES,
        DEFAULT_SSE_MAX_FRAME_BYTES,
        DEFAULT_SSE_MAX_LINE_BYTES,
    },
    redact::{
        LogRedactionPolicy,
        RedactedDebugger,
    },
    request::parse_header,
    sse::{
        DoneMarkerPolicy,
        SseJsonMode,
    },
    HttpResult,
};

/// Aggregated settings for [`crate::HttpClient`] and
/// [`crate::HttpClientFactory`].
#[derive(Clone)]
pub struct HttpClientOptions {
    /// Optional base URL.
    pub base_url: Option<Url>,
    /// Default request headers.
    pub default_headers: HeaderMap,
    /// Timeout options.
    pub timeouts: HttpTimeoutOptions,
    /// Proxy options.
    pub proxy: ProxyOptions,
    /// Logging options.
    pub logging: HttpLoggingOptions,
    /// Maximum bytes captured into `HttpError.response_body_preview` for
    /// non-success responses.
    pub error_response_preview_limit: usize,
    /// Maximum bytes accumulated by [`crate::HttpResponse::bytes`] and its
    /// text/JSON helpers.
    pub response_body_size_limit: usize,
    /// Optional default `User-Agent` header sent by reqwest.
    pub user_agent: Option<String>,
    /// Optional redirect limit applied by reqwest.
    pub max_redirects: Option<usize>,
    /// Optional connection pool idle-time timeout.
    pub pool_idle_timeout: Option<Duration>,
    /// Optional maximum idle connections per host.
    pub pool_max_idle_per_host: Option<usize>,
    /// Whether to inherit proxy settings from environment variables when
    /// explicit proxy config is disabled.
    pub use_env_proxy: bool,
    /// Retry options.
    pub retry: HttpRetryOptions,
    /// Log redaction policy for URL, header, and body previews.
    pub log_redaction_policy: LogRedactionPolicy,
    /// Whether IPv4-only DNS behavior is requested.
    pub ipv4_only: bool,
    /// Default JSON handling mode used by [`crate::HttpResponse::sse_chunks`].
    pub sse_json_mode: SseJsonMode,
    /// Default done-marker policy used by [`crate::HttpResponse::sse_chunks`].
    pub sse_done_marker_policy: DoneMarkerPolicy,
    /// Default maximum bytes for one SSE line.
    pub sse_max_line_bytes: usize,
    /// Default maximum bytes for one SSE frame.
    pub sse_max_frame_bytes: usize,
}

impl Default for HttpClientOptions {
    /// Default: no base URL, empty headers, default timeouts/proxy/logging,
    /// default log redaction, IPv4-only off, lenient SSE JSON mode, default
    /// response-body/SSE limits, and default SSE done-marker policy.
    ///
    /// # Returns
    /// Default [`HttpClientOptions`].
    fn default() -> Self {
        Self {
            base_url: None,
            default_headers: HeaderMap::new(),
            timeouts: HttpTimeoutOptions::default(),
            proxy: ProxyOptions::default(),
            logging: HttpLoggingOptions::default(),
            error_response_preview_limit:
                DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES,
            response_body_size_limit: DEFAULT_RESPONSE_BODY_SIZE_LIMIT_BYTES,
            user_agent: None,
            max_redirects: None,
            pool_idle_timeout: None,
            pool_max_idle_per_host: None,
            use_env_proxy: false,
            retry: HttpRetryOptions::default(),
            log_redaction_policy: LogRedactionPolicy::default(),
            ipv4_only: false,
            sse_json_mode: SseJsonMode::Lenient,
            sse_done_marker_policy: DoneMarkerPolicy::default(),
            sse_max_line_bytes: DEFAULT_SSE_MAX_LINE_BYTES,
            sse_max_frame_bytes: DEFAULT_SSE_MAX_FRAME_BYTES,
        }
    }
}

impl fmt::Debug for HttpClientOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let debugger = RedactedDebugger::new(&self.log_redaction_policy);
        let base_url = debugger.optional_url(self.base_url.as_ref());
        formatter
            .debug_struct("HttpClientOptions")
            .field("base_url", &base_url)
            .field("default_headers", &debugger.headers(&self.default_headers))
            .field("timeouts", &self.timeouts)
            .field("proxy", &self.proxy)
            .field("logging", &self.logging)
            .field(
                "error_response_preview_limit",
                &self.error_response_preview_limit,
            )
            .field("response_body_size_limit", &self.response_body_size_limit)
            .field("user_agent", &self.user_agent)
            .field("max_redirects", &self.max_redirects)
            .field("pool_idle_timeout", &self.pool_idle_timeout)
            .field("pool_max_idle_per_host", &self.pool_max_idle_per_host)
            .field("use_env_proxy", &self.use_env_proxy)
            .field("retry", &self.retry)
            .field("log_redaction_policy", &self.log_redaction_policy)
            .field("ipv4_only", &self.ipv4_only)
            .field("sse_json_mode", &self.sse_json_mode)
            .field("sse_done_marker_policy", &self.sse_done_marker_policy)
            .field("sse_max_line_bytes", &self.sse_max_line_bytes)
            .field("sse_max_frame_bytes", &self.sse_max_frame_bytes)
            .finish()
    }
}

/// Top-level scalar keys read before nested sections and `default_headers`
/// iteration.
struct HttpClientRootConfigInput {
    base_url: Option<String>,
    ipv4_only: Option<bool>,
    error_response_preview_limit: Option<usize>,
    response_body_size_limit: Option<usize>,
    user_agent: Option<String>,
    max_redirects: Option<usize>,
    pool_idle_timeout: Option<Duration>,
    pool_max_idle_per_host: Option<usize>,
    use_env_proxy: Option<bool>,
}

/// SSE scalar keys read from `sse.*`.
struct HttpClientSseConfigInput {
    json_mode: Option<String>,
    done_marker: Option<String>,
    max_line_bytes: Option<usize>,
    max_frame_bytes: Option<usize>,
}

impl HttpClientOptions {
    /// Same as [`HttpClientOptions::default`].
    ///
    /// # Returns
    /// Fresh options with crate defaults.
    #[inline(always)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates [`HttpClientOptions`] from `config` using **relative** keys.
    ///
    /// # Parameters
    /// - `config`: Any [`ConfigReader`] (full [`qubit_config::Config`] or a
    ///   [`qubit_config::ConfigSection`] from [`ConfigReader::section`]).
    ///
    /// # Returns
    /// Parsed options or [`HttpConfigError`].
    ///
    /// # Errors
    /// Returns the first invalid, missing, or incorrectly typed option with
    /// its resolved configuration path.
    #[inline(always)]
    pub fn from_config<R>(config: &R) -> Result<Self, HttpConfigError>
    where
        R: ConfigReader + ?Sized,
    {
        Self::from_config_impl(config)
    }

    /// Implements [`Self::from_config`] after constructor dispatch.
    ///
    /// # Parameters
    /// - `config`: Any [`ConfigReader`] (full [`qubit_config::Config`] or a
    ///   [`qubit_config::ConfigSection`] from [`ConfigReader::section`]).
    ///
    /// # Returns
    /// Parsed options.
    ///
    /// # Errors
    /// Returns the first invalid, missing, or incorrectly typed option with
    /// its resolved configuration path.
    fn from_config_impl<R>(config: &R) -> Result<Self, HttpConfigError>
    where
        R: ConfigReader + ?Sized,
    {
        let mut opts = HttpClientOptions::default();

        let root = match Self::read_config(config) {
            Ok(root) => root,
            Err(error) => {
                return Err(Self::resolve_config_error(
                    config,
                    HttpConfigError::from(error),
                ))
            }
        };

        if let Some(s) = root.base_url {
            if let Err(error) = opts.set_base_url(&s) {
                return Err(Self::resolve_config_error(config, error));
            }
        }

        if let Some(v) = root.ipv4_only {
            opts.ipv4_only = v;
        }
        if let Some(limit) = root.error_response_preview_limit {
            opts.error_response_preview_limit =
                match Self::validate_positive_limit(
                    "error_response_preview_limit",
                    limit,
                ) {
                    Ok(limit) => limit,
                    Err(error) => {
                        return Err(Self::resolve_config_error(config, error))
                    }
                };
        }
        if let Some(limit) = root.response_body_size_limit {
            opts.response_body_size_limit = match Self::validate_positive_limit(
                "response_body_size_limit",
                limit,
            ) {
                Ok(limit) => limit,
                Err(error) => {
                    return Err(Self::resolve_config_error(config, error))
                }
            };
        }
        if let Some(user_agent) = root.user_agent {
            opts.user_agent = Some(user_agent.trim().to_string());
        }
        if let Some(max_redirects) = root.max_redirects {
            opts.max_redirects = Some(max_redirects);
        }
        if let Some(pool_idle_timeout) = root.pool_idle_timeout {
            opts.pool_idle_timeout = Some(pool_idle_timeout);
        }
        if let Some(pool_max_idle_per_host) = root.pool_max_idle_per_host {
            opts.pool_max_idle_per_host = Some(pool_max_idle_per_host);
        }
        if let Some(use_env_proxy) = root.use_env_proxy {
            opts.use_env_proxy = use_env_proxy;
        }

        // timeouts
        if config.contains_section("timeouts") {
            let timeouts_config = config.section("timeouts");
            opts.timeouts =
                match HttpTimeoutOptions::from_config(&timeouts_config) {
                    Ok(timeouts) => timeouts,
                    Err(error) => {
                        return Err(Self::resolve_config_error(
                            &timeouts_config,
                            error,
                        ))
                    }
                };
        }

        // proxy
        if config.contains_section("proxy") {
            let proxy_config = config.section("proxy");
            opts.proxy = match ProxyOptions::from_config(&proxy_config) {
                Ok(proxy) => proxy,
                Err(error) => {
                    return Err(Self::resolve_config_error(
                        &proxy_config,
                        error,
                    ))
                }
            };
        }

        // logging
        if config.contains_section("logging") {
            let logging_config = config.section("logging");
            opts.logging =
                match HttpLoggingOptions::from_config(&logging_config) {
                    Ok(logging) => logging,
                    Err(error) => {
                        return Err(Self::resolve_config_error(
                            &logging_config,
                            error,
                        ))
                    }
                };
        }

        if config.contains_section("retry") {
            let retry_config = config.section("retry");
            opts.retry = match HttpRetryOptions::from_config(&retry_config) {
                Ok(retry) => retry,
                Err(error) => {
                    return Err(Self::resolve_config_error(
                        &retry_config,
                        error,
                    ))
                }
            };
        }

        if config.contains_section("sse") {
            let sse_config = config.section("sse");
            let sse = match Self::read_sse_config(&sse_config) {
                Ok(sse) => sse,
                Err(error) => {
                    return Err(Self::resolve_config_error(
                        &sse_config,
                        HttpConfigError::from(error),
                    ))
                }
            };
            if let Some(mode) = sse.json_mode.as_deref() {
                opts.sse_json_mode = match Self::parse_sse_json_mode(mode) {
                    Ok(mode) => mode,
                    Err(error) => {
                        return Err(Self::resolve_config_error(
                            &sse_config,
                            error,
                        ))
                    }
                };
            }
            if let Some(marker) = sse.done_marker.as_deref() {
                opts.sse_done_marker_policy =
                    match Self::parse_sse_done_marker_policy(marker) {
                        Ok(marker) => marker,
                        Err(error) => {
                            return Err(Self::resolve_config_error(
                                &sse_config,
                                error,
                            ))
                        }
                    };
            }
            if let Some(max_line_bytes) = sse.max_line_bytes {
                opts.sse_max_line_bytes = match Self::validate_positive_limit(
                    "max_line_bytes",
                    max_line_bytes,
                ) {
                    Ok(limit) => limit,
                    Err(error) => {
                        return Err(Self::resolve_config_error(
                            &sse_config,
                            error,
                        ))
                    }
                };
            }
            if let Some(max_frame_bytes) = sse.max_frame_bytes {
                opts.sse_max_frame_bytes = match Self::validate_positive_limit(
                    "max_frame_bytes",
                    max_frame_bytes,
                ) {
                    Ok(limit) => limit,
                    Err(error) => {
                        return Err(Self::resolve_config_error(
                            &sse_config,
                            error,
                        ))
                    }
                };
            }
        }

        if config.contains_section("log_redaction") {
            let log_redaction_config = config.section("log_redaction");
            let log_redaction =
                match Self::read_log_redaction_config(&log_redaction_config) {
                    Ok(log_redaction) => log_redaction,
                    Err(error) => {
                        return Err(Self::resolve_config_error(
                            &log_redaction_config,
                            HttpConfigError::from(error),
                        ))
                    }
                };
            let mut policy_builder = LogRedactionPolicy::builder();
            if let Some(value) = log_redaction.url_path_policy.as_deref() {
                let policy = match Self::parse_url_path_policy(value) {
                    Ok(policy) => policy,
                    Err(error) => {
                        return Err(Self::resolve_config_error(
                            &log_redaction_config,
                            error,
                        ))
                    }
                };
                policy_builder = policy_builder.url_path_policy(policy);
            }
            if let Some(names) = log_redaction.sensitive_headers {
                for name in names {
                    Self::validate_redaction_field("sensitive_headers", &name)
                        .map_err(|error| {
                            Self::resolve_config_error(
                                &log_redaction_config,
                                error,
                            )
                        })?;
                    policy_builder =
                        policy_builder.raise_header(&name, Sensitivity::High);
                }
            }
            if let Some(names) = log_redaction.sensitive_query_params {
                for name in names {
                    Self::validate_redaction_field(
                        "sensitive_query_params",
                        &name,
                    )
                    .map_err(|error| {
                        Self::resolve_config_error(&log_redaction_config, error)
                    })?;
                    policy_builder =
                        policy_builder.raise_query(&name, Sensitivity::High);
                }
            }
            if let Some(names) = log_redaction.sensitive_body_fields {
                for name in names {
                    Self::validate_redaction_field(
                        "sensitive_body_fields",
                        &name,
                    )
                    .map_err(|error| {
                        Self::resolve_config_error(&log_redaction_config, error)
                    })?;
                    policy_builder =
                        policy_builder.raise_body(&name, Sensitivity::High);
                }
            }
            if let Some(names) = log_redaction.excluded_sensitive_headers {
                for name in names {
                    Self::validate_redaction_field(
                        "excluded_sensitive_headers",
                        &name,
                    )
                    .map_err(|error| {
                        Self::resolve_config_error(&log_redaction_config, error)
                    })?;
                    policy_builder = policy_builder.allow_header_exact(&name);
                }
            }
            if let Some(names) = log_redaction.excluded_sensitive_query_params {
                for name in names {
                    Self::validate_redaction_field(
                        "excluded_sensitive_query_params",
                        &name,
                    )
                    .map_err(|error| {
                        Self::resolve_config_error(&log_redaction_config, error)
                    })?;
                    policy_builder = policy_builder.allow_query_exact(&name);
                }
            }
            if let Some(names) = log_redaction.excluded_sensitive_body_fields {
                for name in names {
                    Self::validate_redaction_field(
                        "excluded_sensitive_body_fields",
                        &name,
                    )
                    .map_err(|error| {
                        Self::resolve_config_error(&log_redaction_config, error)
                    })?;
                    policy_builder = policy_builder.allow_body_exact(&name);
                }
            }
            opts.log_redaction_policy =
                policy_builder.build().map_err(|error| {
                    Self::resolve_config_error(
                        &log_redaction_config,
                        HttpConfigError::invalid_value(
                            "log_redaction",
                            error.to_string(),
                        ),
                    )
                })?;
        }

        // default_headers – sub-key form: default_headers.<name> = <value>
        let headers_prefix = "default_headers";
        let full_headers_prefix = "default_headers.";
        let mut header_map: HashMap<String, String> = HashMap::new();
        for (k, _) in config.iter_prefix(full_headers_prefix) {
            let header_name = &k[full_headers_prefix.len()..];
            let value = match config.get_interpolated::<String>(k) {
                Ok(value) => value,
                Err(error) => {
                    return Err(HttpConfigError::config_error(
                        config.resolve_key(k),
                        error.to_string(),
                    ))
                }
            };
            header_map.insert(header_name.to_string(), value);
        }
        // Also support JSON map form stored at the exact key `default_headers`.
        let json_headers =
            match config.get_optional_interpolated::<String>(headers_prefix) {
                Ok(json_headers) => json_headers,
                Err(error) => {
                    return Err(Self::resolve_config_error(
                        config,
                        HttpConfigError::from(error),
                    ))
                }
            };
        if !header_map.is_empty() && json_headers.is_some() {
            return Err(HttpConfigError::invalid_value(
                config.resolve_key(headers_prefix),
                "default_headers sub-key form and JSON map form cannot be used at the same time",
            ));
        }
        if let Some(json_str) = json_headers {
            let parsed: HashMap<String, String> =
                match serde_json::from_str(&json_str) {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        return Err(HttpConfigError::type_error(
                            config.resolve_key(headers_prefix),
                            format!(
                                "Failed to parse default_headers JSON: {error}"
                            ),
                        ))
                    }
                };
            header_map = parsed;
        }
        if !header_map.is_empty() {
            opts.default_headers = hashmap_to_headermap(
                &config.resolve_key(headers_prefix),
                header_map,
            )?;
        }

        Ok(opts)
    }

    /// Parses and sets the base URL used to resolve relative request paths.
    ///
    /// # Parameters
    /// - `base_url`: Absolute base URL string.
    ///
    /// # Returns
    /// `Ok(self)` or [`HttpConfigError`] if the URL is invalid.
    #[inline]
    pub fn set_base_url(
        &mut self,
        base_url: &str,
    ) -> Result<&mut Self, HttpConfigError> {
        let parsed = Self::parse_base_url(base_url)?;
        self.base_url = Some(parsed);
        Ok(self)
    }

    /// Validates and adds one client-level default header.
    ///
    /// # Parameters
    /// - `name`: Header name.
    /// - `value`: Header value.
    ///
    /// # Returns
    /// `Ok(self)` or an error if name/value are invalid.
    #[inline]
    pub fn add_header(
        &mut self,
        name: &str,
        value: &str,
    ) -> HttpResult<&mut Self> {
        let (header_name, header_value) = parse_header(name, value)?;
        self.default_headers.insert(header_name, header_value);
        Ok(self)
    }

    /// Validates and adds many client-level default headers atomically.
    ///
    /// If any input pair is invalid, no header from this batch is applied.
    ///
    /// # Parameters
    /// - `headers`: Iterator of `(name, value)` pairs.
    ///
    /// # Returns
    /// `Ok(self)` or an error if any pair is invalid.
    pub fn add_headers(
        &mut self,
        headers: &[(&str, &str)],
    ) -> HttpResult<&mut Self> {
        let mut parsed_headers = HeaderMap::new();
        for &(name, value) in headers {
            let (header_name, header_value) = parse_header(name, value)?;
            parsed_headers.insert(header_name, header_value);
        }
        self.default_headers.extend(parsed_headers);
        Ok(self)
    }

    /// Runs [`ProxyOptions::validate`], [`HttpLoggingOptions::validate`], retry
    /// validation, and SSE limit validation.
    ///
    /// # Returns
    /// `Ok(())` or the first sub-validator error.
    pub fn validate(&self) -> Result<(), HttpConfigError> {
        self.timeouts
            .validate_arguments()
            .with_path_prefix("timeouts")?;
        self.proxy.validate_arguments()?;
        self.logging.validate_arguments()?;
        self.retry.validate_arguments().with_path_prefix("retry")?;
        Self::validate_positive_limit(
            "error_response_preview_limit",
            self.error_response_preview_limit,
        )?;
        Self::validate_positive_limit(
            "response_body_size_limit",
            self.response_body_size_limit,
        )?;
        if let Some(user_agent) = self.user_agent.as_deref() {
            require_that(
                user_agent,
                "user_agent",
                |value| !value.trim().is_empty(),
                "blank_user_agent",
                "Value cannot be empty",
            )?;
            HeaderValue::from_str(user_agent).map_err(|error| {
                HttpConfigError::invalid_value(
                    "user_agent",
                    format!("Invalid header value: {error}"),
                )
            })?;
        }
        Self::validate_positive_limit(
            "sse.max_line_bytes",
            self.sse_max_line_bytes,
        )?;
        Self::validate_positive_limit(
            "sse.max_frame_bytes",
            self.sse_max_frame_bytes,
        )?;
        Ok(())
    }

    fn resolve_config_error<R>(
        config: &R,
        mut error: HttpConfigError,
    ) -> HttpConfigError
    where
        R: ConfigReader + ?Sized,
    {
        let section_path = config.resolve_key("");
        error.path = if error.path.is_empty() {
            section_path
        } else if section_path.is_empty()
            || error.path == section_path
            || error
                .path
                .strip_prefix(&section_path)
                .is_some_and(|suffix| suffix.starts_with('.'))
        {
            error.path
        } else {
            config.resolve_key(&error.path)
        };
        error
    }

    fn read_config<R>(config: &R) -> ConfigResult<HttpClientRootConfigInput>
    where
        R: ConfigReader + ?Sized,
    {
        Ok(HttpClientRootConfigInput {
            base_url: config.get_optional_interpolated::<String>("base_url")?,
            ipv4_only: config.get_optional("ipv4_only")?,
            error_response_preview_limit: get_optional_usize(
                config,
                "error_response_preview_limit",
            )?,
            response_body_size_limit: get_optional_usize(
                config,
                "response_body_size_limit",
            )?,
            user_agent: config
                .get_optional_interpolated::<String>("user_agent")?,
            max_redirects: get_optional_usize(config, "max_redirects")?,
            pool_idle_timeout: config.get_optional("pool_idle_timeout")?,
            pool_max_idle_per_host: get_optional_usize(
                config,
                "pool_max_idle_per_host",
            )?,
            use_env_proxy: config.get_optional("use_env_proxy")?,
        })
    }

    fn read_sse_config<R>(config: &R) -> ConfigResult<HttpClientSseConfigInput>
    where
        R: ConfigReader + ?Sized,
    {
        Ok(HttpClientSseConfigInput {
            json_mode: config
                .get_optional_interpolated::<String>("json_mode")?,
            done_marker: config
                .get_optional_interpolated::<String>("done_marker")?,
            max_line_bytes: get_optional_usize(config, "max_line_bytes")?,
            max_frame_bytes: get_optional_usize(config, "max_frame_bytes")?,
        })
    }

    /// Reads unvalidated values from a `log_redaction` configuration section.
    ///
    /// # Parameters
    ///
    /// * `config` - Reader scoped to the `log_redaction` section.
    ///
    /// # Returns
    ///
    /// Optional path behavior, sensitive-name lists, and allow-name lists.
    ///
    /// # Errors
    ///
    /// Returns the first [`qubit_config::ConfigError`] produced while reading
    /// or interpolating a configured value.
    fn read_log_redaction_config<R>(
        config: &R,
    ) -> ConfigResult<HttpClientLogRedactionConfigInput>
    where
        R: ConfigReader + ?Sized,
    {
        Ok(HttpClientLogRedactionConfigInput {
            url_path_policy: config
                .get_optional_interpolated::<String>("url_path_policy")?,
            sensitive_headers: config
                .get_optional_interpolated::<Vec<String>>(
                    "sensitive_headers",
                )?,
            sensitive_query_params: config
                .get_optional_interpolated::<Vec<String>>(
                    "sensitive_query_params",
                )?,
            sensitive_body_fields: config
                .get_optional_interpolated::<Vec<String>>(
                    "sensitive_body_fields",
                )?,
            excluded_sensitive_headers: config
                .get_optional_interpolated::<Vec<String>>(
                    "excluded_sensitive_headers",
                )?,
            excluded_sensitive_query_params: config
                .get_optional_interpolated::<Vec<String>>(
                    "excluded_sensitive_query_params",
                )?,
            excluded_sensitive_body_fields: config
                .get_optional_interpolated::<Vec<String>>(
                    "excluded_sensitive_body_fields",
                )?,
        })
    }

    fn parse_sse_done_marker_policy(
        value: &str,
    ) -> Result<DoneMarkerPolicy, HttpConfigError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(HttpConfigError::invalid_value(
                "done_marker",
                "Value must not be empty",
            ));
        }
        Ok(DoneMarkerPolicy::from_str(trimmed).expect(
            "DoneMarkerPolicy::from_str accepts arbitrary custom markers",
        ))
    }

    fn parse_base_url(base_url: &str) -> Result<Url, HttpConfigError> {
        Url::parse(base_url).map_err(|error| {
            HttpConfigError::invalid_value(
                "base_url",
                format!("Invalid URL: {error}"),
            )
        })
    }

    fn parse_sse_json_mode(
        value: &str,
    ) -> Result<SseJsonMode, HttpConfigError> {
        SseJsonMode::from_str(value.trim()).map_err(|_| {
            HttpConfigError::invalid_value(
                "json_mode",
                format!("Unsupported SSE JSON mode: {value}"),
            )
        })
    }

    /// Parses the URL path rendering policy from configuration text.
    ///
    /// # Parameters
    ///
    /// * `value` - Configured `redact` or `preserve` value.
    ///
    /// # Returns
    ///
    /// Parsed URL path policy.
    ///
    /// # Errors
    ///
    /// Returns [`HttpConfigError`] when the value is unsupported.
    fn parse_url_path_policy(
        value: &str,
    ) -> Result<UrlPathPolicy, HttpConfigError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "redact" => Ok(UrlPathPolicy::Redact),
            "preserve" => Ok(UrlPathPolicy::Preserve),
            _ => Err(HttpConfigError::invalid_value(
                "url_path_policy",
                format!("Unsupported URL path policy: {value}"),
            )),
        }
    }

    /// Validates one configured redaction field name.
    ///
    /// # Parameters
    ///
    /// * `field` - Concrete configuration list key for error reporting.
    /// * `name` - Candidate field name from that list.
    ///
    /// # Returns
    ///
    /// `Ok(())` when the runtime policy accepts the field name.
    ///
    /// # Errors
    ///
    /// Returns [`HttpConfigError`] carrying `field` when the name
    /// canonicalizes to an invalid runtime rule.
    fn validate_redaction_field(
        field: &str,
        name: &str,
    ) -> Result<(), HttpConfigError> {
        RedactionPolicy::builder()
            .raise(name, Sensitivity::Low)
            .build()
            .map(|_| ())
            .map_err(|error| {
                HttpConfigError::invalid_value(field, error.to_string())
            })
    }

    fn validate_positive_limit(
        path: &str,
        value: usize,
    ) -> Result<usize, HttpConfigError> {
        Ok(require_that(
            value,
            path,
            |limit| *limit > 0,
            "positive_limit",
            "Value must be greater than 0",
        )?)
    }
}
