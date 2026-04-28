/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::collections::HashMap;
use std::time::Duration;

use http::HeaderMap;
use http::HeaderValue;
use qubit_config::{ConfigReader, ConfigResult};
use std::str::FromStr;
use url::Url;

use super::from_config_helpers::hashmap_to_headermap;
use super::http_logging_options::HttpLoggingOptions;
use super::http_retry_options::HttpRetryOptions;
use super::http_timeout_options::HttpTimeoutOptions;
use super::proxy_options::ProxyOptions;
use super::sensitive_http_headers::SensitiveHttpHeaders;
use super::HttpConfigError;
use crate::{
    constants::{
        DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES, DEFAULT_SSE_MAX_FRAME_BYTES,
        DEFAULT_SSE_MAX_LINE_BYTES,
    },
    request::parse_header,
    sse::{DoneMarkerPolicy, SseJsonMode},
    HttpResult,
};

/// Aggregated settings for [`crate::HttpClient`] and [`crate::HttpClientFactory`].
#[derive(Debug, Clone)]
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
    /// Maximum bytes captured into `HttpError.response_body_preview` for non-success responses.
    pub error_response_preview_limit: usize,
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
    /// Sensitive headers for masking.
    pub sensitive_headers: SensitiveHttpHeaders,
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
    /// default sensitive headers, IPv4-only off, lenient SSE JSON mode, default SSE done-marker
    /// policy, and crate default SSE line/frame limits.
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
            error_response_preview_limit: DEFAULT_ERROR_RESPONSE_PREVIEW_LIMIT_BYTES,
            user_agent: None,
            max_redirects: None,
            pool_idle_timeout: None,
            pool_max_idle_per_host: None,
            use_env_proxy: false,
            retry: HttpRetryOptions::default(),
            sensitive_headers: SensitiveHttpHeaders::default(),
            ipv4_only: false,
            sse_json_mode: SseJsonMode::Lenient,
            sse_done_marker_policy: DoneMarkerPolicy::default(),
            sse_max_line_bytes: DEFAULT_SSE_MAX_LINE_BYTES,
            sse_max_frame_bytes: DEFAULT_SSE_MAX_FRAME_BYTES,
        }
    }
}

/// Top-level scalar keys read before nested sections and `default_headers` iteration.
struct HttpClientRootConfigInput {
    base_url: Option<String>,
    ipv4_only: Option<bool>,
    error_response_preview_limit: Option<usize>,
    user_agent: Option<String>,
    max_redirects: Option<usize>,
    pool_idle_timeout: Option<Duration>,
    pool_max_idle_per_host: Option<usize>,
    use_env_proxy: Option<bool>,
    sensitive_headers: Option<Vec<String>>,
}

/// SSE scalar keys read from `sse.*`.
struct HttpClientSseConfigInput {
    json_mode: Option<String>,
    done_marker: Option<String>,
    max_line_bytes: Option<usize>,
    max_frame_bytes: Option<usize>,
}

impl HttpClientOptions {
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

    fn read_config<R>(config: &R) -> ConfigResult<HttpClientRootConfigInput>
    where
        R: ConfigReader + ?Sized,
    {
        Ok(HttpClientRootConfigInput {
            base_url: config.get_optional_string("base_url")?,
            ipv4_only: config.get_optional("ipv4_only")?,
            error_response_preview_limit: config.get_optional("error_response_preview_limit")?,
            user_agent: config.get_optional_string("user_agent")?,
            max_redirects: config.get_optional("max_redirects")?,
            pool_idle_timeout: config.get_optional("pool_idle_timeout")?,
            pool_max_idle_per_host: config.get_optional("pool_max_idle_per_host")?,
            use_env_proxy: config.get_optional("use_env_proxy")?,
            sensitive_headers: config.get_optional_string_list("sensitive_headers")?,
        })
    }

    fn read_sse_config<R>(config: &R) -> ConfigResult<HttpClientSseConfigInput>
    where
        R: ConfigReader + ?Sized,
    {
        Ok(HttpClientSseConfigInput {
            json_mode: config.get_optional_string("json_mode")?,
            done_marker: config.get_optional_string("done_marker")?,
            max_line_bytes: config.get_optional("max_line_bytes")?,
            max_frame_bytes: config.get_optional("max_frame_bytes")?,
        })
    }

    fn parse_sse_done_marker_policy(value: &str) -> Result<DoneMarkerPolicy, HttpConfigError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(HttpConfigError::invalid_value(
                "done_marker",
                "Value must not be empty",
            ));
        }
        Ok(DoneMarkerPolicy::from_str(trimmed)
            .expect("DoneMarkerPolicy::from_str accepts arbitrary custom markers"))
    }

    fn parse_base_url(base_url: &str) -> Result<Url, HttpConfigError> {
        Url::parse(base_url).map_err(|error| {
            HttpConfigError::invalid_value("base_url", format!("Invalid URL: {error}"))
        })
    }

    fn parse_sse_json_mode(value: &str) -> Result<SseJsonMode, HttpConfigError> {
        SseJsonMode::from_str(value.trim()).map_err(|_| {
            HttpConfigError::invalid_value(
                "json_mode",
                format!("Unsupported SSE JSON mode: {value}"),
            )
        })
    }

    fn validate_positive_limit(path: &str, value: usize) -> Result<usize, HttpConfigError> {
        if value == 0 {
            return Err(HttpConfigError::invalid_value(
                path,
                "Value must be greater than 0",
            ));
        }
        Ok(value)
    }

    /// Same as [`HttpClientOptions::default`].
    ///
    /// # Returns
    /// Fresh options with crate defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses and sets the base URL used to resolve relative request paths.
    ///
    /// # Parameters
    /// - `base_url`: Absolute base URL string.
    ///
    /// # Returns
    /// `Ok(self)` or [`HttpConfigError`] if the URL is invalid.
    pub fn set_base_url(&mut self, base_url: &str) -> Result<&mut Self, HttpConfigError> {
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
    pub fn add_header(&mut self, name: &str, value: &str) -> HttpResult<&mut Self> {
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
    pub fn add_headers(&mut self, headers: &[(&str, &str)]) -> HttpResult<&mut Self> {
        let mut parsed_headers = HeaderMap::new();
        for &(name, value) in headers {
            let (header_name, header_value) = parse_header(name, value)?;
            parsed_headers.insert(header_name, header_value);
        }
        self.default_headers.extend(parsed_headers);
        Ok(self)
    }

    /// Creates [`HttpClientOptions`] from `config` using **relative** keys.
    ///
    /// # Parameters
    /// - `config`: Any [`ConfigReader`] (full [`qubit_config::Config`] or a
    ///   [`qubit_config::ConfigPrefixView`] from [`ConfigReader::prefix_view`]).
    ///
    /// # Returns
    /// Parsed options or [`HttpConfigError`].
    pub fn from_config<R>(config: &R) -> Result<Self, HttpConfigError>
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
                match Self::validate_positive_limit("error_response_preview_limit", limit) {
                    Ok(limit) => limit,
                    Err(error) => return Err(Self::resolve_config_error(config, error)),
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
        if config.contains_prefix("timeouts") {
            let timeouts_config = config.prefix_view("timeouts");
            opts.timeouts = match HttpTimeoutOptions::from_config(&timeouts_config) {
                Ok(timeouts) => timeouts,
                Err(error) => return Err(Self::resolve_config_error(&timeouts_config, error)),
            };
        }

        // proxy
        if config.contains_prefix("proxy") {
            let proxy_config = config.prefix_view("proxy");
            opts.proxy = match ProxyOptions::from_config(&proxy_config) {
                Ok(proxy) => proxy,
                Err(error) => return Err(Self::resolve_config_error(&proxy_config, error)),
            };
        }

        // logging
        if config.contains_prefix("logging") {
            let logging_config = config.prefix_view("logging");
            opts.logging = match HttpLoggingOptions::from_config(&logging_config) {
                Ok(logging) => logging,
                Err(error) => return Err(Self::resolve_config_error(&logging_config, error)),
            };
        }

        if config.contains_prefix("retry") {
            let retry_config = config.prefix_view("retry");
            opts.retry = match HttpRetryOptions::from_config(&retry_config) {
                Ok(retry) => retry,
                Err(error) => return Err(Self::resolve_config_error(&retry_config, error)),
            };
        }

        if config.contains_prefix("sse") {
            let sse_config = config.prefix_view("sse");
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
                    Err(error) => return Err(Self::resolve_config_error(&sse_config, error)),
                };
            }
            if let Some(marker) = sse.done_marker.as_deref() {
                opts.sse_done_marker_policy = match Self::parse_sse_done_marker_policy(marker) {
                    Ok(marker) => marker,
                    Err(error) => return Err(Self::resolve_config_error(&sse_config, error)),
                };
            }
            if let Some(max_line_bytes) = sse.max_line_bytes {
                opts.sse_max_line_bytes =
                    match Self::validate_positive_limit("max_line_bytes", max_line_bytes) {
                        Ok(limit) => limit,
                        Err(error) => return Err(Self::resolve_config_error(&sse_config, error)),
                    };
            }
            if let Some(max_frame_bytes) = sse.max_frame_bytes {
                opts.sse_max_frame_bytes =
                    match Self::validate_positive_limit("max_frame_bytes", max_frame_bytes) {
                        Ok(limit) => limit,
                        Err(error) => return Err(Self::resolve_config_error(&sse_config, error)),
                    };
            }
        }

        // default_headers – sub-key form: default_headers.<name> = <value>
        let headers_prefix = "default_headers";
        let full_headers_prefix = "default_headers.";
        let mut header_map: HashMap<String, String> = HashMap::new();
        for (k, _) in config.iter_prefix(full_headers_prefix) {
            let header_name = &k[full_headers_prefix.len()..];
            let value = match config.get_string(k) {
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
        let json_headers = match config.get_optional_string(headers_prefix) {
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
            let parsed: HashMap<String, String> = match serde_json::from_str(&json_str) {
                Ok(parsed) => parsed,
                Err(error) => {
                    return Err(HttpConfigError::type_error(
                        config.resolve_key(headers_prefix),
                        format!("Failed to parse default_headers JSON: {error}"),
                    ))
                }
            };
            header_map = parsed;
        }
        if !header_map.is_empty() {
            opts.default_headers =
                hashmap_to_headermap(&config.resolve_key(headers_prefix), header_map)?;
        }

        if let Some(names) = root.sensitive_headers {
            let mut sh = SensitiveHttpHeaders::new();
            sh.extend(names);
            opts.sensitive_headers = sh;
        }

        Ok(opts)
    }

    /// Runs [`ProxyOptions::validate`], [`HttpLoggingOptions::validate`], retry validation,
    /// and SSE limit validation.
    ///
    /// # Returns
    /// `Ok(())` or the first sub-validator error.
    pub fn validate(&self) -> Result<(), HttpConfigError> {
        self.timeouts
            .validate()
            .map_err(|e| e.prepend_path_prefix("timeouts"))?;
        self.proxy.validate()?;
        self.logging.validate()?;
        self.retry
            .validate()
            .map_err(|e| e.prepend_path_prefix("retry"))?;
        Self::validate_positive_limit(
            "error_response_preview_limit",
            self.error_response_preview_limit,
        )?;
        if let Some(user_agent) = self.user_agent.as_deref() {
            if user_agent.trim().is_empty() {
                return Err(HttpConfigError::invalid_value(
                    "user_agent",
                    "Value cannot be empty",
                ));
            }
            HeaderValue::from_str(user_agent).map_err(|error| {
                HttpConfigError::invalid_value(
                    "user_agent",
                    format!("Invalid header value: {error}"),
                )
            })?;
        }
        Self::validate_positive_limit("sse.max_line_bytes", self.sse_max_line_bytes)?;
        Self::validate_positive_limit("sse.max_frame_bytes", self.sse_max_frame_bytes)?;
        Ok(())
    }
}

/// Exercises internal option parser branches for coverage-only tests.
///
/// # Returns
/// Diagnostic strings proving private config helpers and validation closures ran.
#[cfg(coverage)]
#[doc(hidden)]
pub(crate) fn coverage_exercise_http_client_option_paths() -> Vec<String> {
    let config = qubit_config::Config::new();
    let scoped_error = HttpClientOptions::resolve_config_error(
        &config.prefix_view("coverage"),
        HttpConfigError::invalid_value("", "coverage error"),
    );
    let root = HttpClientOptions::read_config(&config.prefix_view("coverage"))
        .expect("empty root config should read");
    let sse = HttpClientOptions::read_sse_config(&config.prefix_view("coverage.sse"))
        .expect("empty SSE config should read");
    let custom_marker = HttpClientOptions::parse_sse_done_marker_policy("coverage-done")
        .expect("custom done marker should parse");
    let mut options = HttpClientOptions {
        user_agent: Some("bad\nagent".to_string()),
        ..HttpClientOptions::default()
    };
    let invalid_user_agent = options
        .validate()
        .expect_err("invalid user agent should fail")
        .message;
    options.user_agent = Some("coverage-agent".to_string());
    options
        .add_headers(&[("x-coverage-a", "a"), ("x-coverage-b", "b")])
        .expect("coverage headers should parse");

    vec![
        scoped_error.path,
        root.base_url.is_none().to_string(),
        sse.json_mode.is_none().to_string(),
        format!("{custom_marker}"),
        invalid_user_agent,
        options.default_headers.len().to_string(),
    ]
}
