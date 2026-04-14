/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::collections::HashMap;

use http::HeaderMap;
use qubit_config::{ConfigReader, ConfigResult};
use url::Url;

use super::from_config_helpers::hashmap_to_headermap;
use super::http_retry_options::HttpRetryOptions;
use super::logging_options::HttpLoggingOptions;
use super::proxy_options::ProxyOptions;
use super::sensitive_headers::SensitiveHeaders;
use super::timeout_options::TimeoutOptions;
use super::HttpConfigError;
use crate::{
    constants::{DEFAULT_SSE_MAX_FRAME_BYTES, DEFAULT_SSE_MAX_LINE_BYTES},
    request::parse_header,
    sse::SseJsonMode,
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
    pub timeouts: TimeoutOptions,
    /// Proxy options.
    pub proxy: ProxyOptions,
    /// Logging options.
    pub logging: HttpLoggingOptions,
    /// Retry options.
    pub retry: HttpRetryOptions,
    /// Sensitive headers for masking.
    pub sensitive_headers: SensitiveHeaders,
    /// Whether IPv4-only DNS behavior is requested.
    pub ipv4_only: bool,
    /// Default JSON handling mode used by [`crate::HttpStreamResponse::decode_json_chunks`].
    pub sse_json_mode: SseJsonMode,
    /// Default maximum bytes for one SSE line.
    pub sse_max_line_bytes: usize,
    /// Default maximum bytes for one SSE frame.
    pub sse_max_frame_bytes: usize,
}

impl Default for HttpClientOptions {
    /// Default: no base URL, empty headers, default timeouts/proxy/logging,
    /// default sensitive headers, IPv4-only off, lenient SSE JSON mode with
    /// crate default SSE line/frame limits.
    ///
    /// # Returns
    /// Default [`HttpClientOptions`].
    fn default() -> Self {
        Self {
            base_url: None,
            default_headers: HeaderMap::new(),
            timeouts: TimeoutOptions::default(),
            proxy: ProxyOptions::default(),
            logging: HttpLoggingOptions::default(),
            retry: HttpRetryOptions::default(),
            sensitive_headers: SensitiveHeaders::default(),
            ipv4_only: false,
            sse_json_mode: SseJsonMode::Lenient,
            sse_max_line_bytes: DEFAULT_SSE_MAX_LINE_BYTES,
            sse_max_frame_bytes: DEFAULT_SSE_MAX_FRAME_BYTES,
        }
    }
}

/// Top-level scalar keys read before nested sections and `default_headers` iteration.
struct HttpClientRootConfigInput {
    base_url: Option<String>,
    ipv4_only: Option<bool>,
    sensitive_headers: Option<Vec<String>>,
}

/// SSE scalar keys read from `sse.*`.
struct HttpClientSseConfigInput {
    json_mode: Option<String>,
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
            sensitive_headers: config.get_optional_string_list("sensitive_headers")?,
        })
    }

    fn read_sse_config<R>(config: &R) -> ConfigResult<HttpClientSseConfigInput>
    where
        R: ConfigReader + ?Sized,
    {
        Ok(HttpClientSseConfigInput {
            json_mode: config.get_optional_string("json_mode")?,
            max_line_bytes: config.get_optional("max_line_bytes")?,
            max_frame_bytes: config.get_optional("max_frame_bytes")?,
        })
    }

    fn parse_base_url(base_url: &str) -> Result<Url, HttpConfigError> {
        Url::parse(base_url).map_err(|error| {
            HttpConfigError::invalid_value("base_url", format!("Invalid URL: {error}"))
        })
    }

    fn parse_sse_json_mode(value: &str) -> Result<SseJsonMode, HttpConfigError> {
        let normalized = value.trim().to_ascii_uppercase().replace('-', "_");
        match normalized.as_str() {
            "LENIENT" => Ok(SseJsonMode::Lenient),
            "STRICT" => Ok(SseJsonMode::Strict),
            _ => Err(HttpConfigError::invalid_value(
                "json_mode",
                format!("Unsupported SSE JSON mode: {value}"),
            )),
        }
    }

    fn validate_sse_limit(path: &str, value: usize) -> Result<usize, HttpConfigError> {
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
    pub fn add_headers<'a, I>(&mut self, headers: I) -> HttpResult<&mut Self>
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        let mut parsed_headers = HeaderMap::new();
        for (name, value) in headers {
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

        let root = Self::read_config(config)
            .map_err(HttpConfigError::from)
            .map_err(|e| Self::resolve_config_error(config, e))?;

        if let Some(s) = root.base_url {
            opts.set_base_url(&s)
                .map_err(|e| Self::resolve_config_error(config, e))?;
        }

        if let Some(v) = root.ipv4_only {
            opts.ipv4_only = v;
        }

        // timeouts
        if config.contains_prefix("timeouts") {
            let timeouts_config = config.prefix_view("timeouts");
            opts.timeouts = TimeoutOptions::from_config(&timeouts_config)
                .map_err(|e| Self::resolve_config_error(&timeouts_config, e))?;
        }

        // proxy
        if config.contains_prefix("proxy") {
            let proxy_config = config.prefix_view("proxy");
            opts.proxy = ProxyOptions::from_config(&proxy_config)
                .map_err(|e| Self::resolve_config_error(&proxy_config, e))?;
        }

        // logging
        if config.contains_prefix("logging") {
            let logging_config = config.prefix_view("logging");
            opts.logging = HttpLoggingOptions::from_config(&logging_config)
                .map_err(|e| Self::resolve_config_error(&logging_config, e))?;
        }

        if config.contains_prefix("retry") {
            let retry_config = config.prefix_view("retry");
            opts.retry = HttpRetryOptions::from_config(&retry_config)
                .map_err(|e| Self::resolve_config_error(&retry_config, e))?;
        }

        if config.contains_prefix("sse") {
            let sse_config = config.prefix_view("sse");
            let sse = Self::read_sse_config(&sse_config)
                .map_err(HttpConfigError::from)
                .map_err(|e| Self::resolve_config_error(&sse_config, e))?;
            if let Some(mode) = sse.json_mode.as_deref() {
                opts.sse_json_mode = Self::parse_sse_json_mode(mode)
                    .map_err(|e| Self::resolve_config_error(&sse_config, e))?;
            }
            if let Some(max_line_bytes) = sse.max_line_bytes {
                opts.sse_max_line_bytes =
                    Self::validate_sse_limit("max_line_bytes", max_line_bytes)
                        .map_err(|e| Self::resolve_config_error(&sse_config, e))?;
            }
            if let Some(max_frame_bytes) = sse.max_frame_bytes {
                opts.sse_max_frame_bytes =
                    Self::validate_sse_limit("max_frame_bytes", max_frame_bytes)
                        .map_err(|e| Self::resolve_config_error(&sse_config, e))?;
            }
        }

        // default_headers – sub-key form: default_headers.<name> = <value>
        let headers_prefix = "default_headers";
        let full_headers_prefix = "default_headers.";
        let mut header_map: HashMap<String, String> = HashMap::new();
        for (k, _) in config.iter_prefix(full_headers_prefix) {
            let header_name = &k[full_headers_prefix.len()..];
            let value = config
                .get_string(k)
                .map_err(|e| HttpConfigError::config_error(config.resolve_key(k), e.to_string()))?;
            header_map.insert(header_name.to_string(), value);
        }
        // Also support JSON map form stored at the exact key `default_headers`.
        if header_map.is_empty() {
            if let Some(json_str) = config
                .get_optional_string(headers_prefix)
                .map_err(HttpConfigError::from)
                .map_err(|e| Self::resolve_config_error(config, e))?
            {
                let parsed: HashMap<String, String> =
                    serde_json::from_str(&json_str).map_err(|e| {
                        HttpConfigError::type_error(
                            config.resolve_key(headers_prefix),
                            format!("Failed to parse default_headers JSON: {e}"),
                        )
                    })?;
                header_map = parsed;
            }
        }
        if !header_map.is_empty() {
            opts.default_headers = hashmap_to_headermap(headers_prefix, header_map)?;
        }

        if let Some(names) = root.sensitive_headers {
            let mut sh = SensitiveHeaders::new();
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
        Self::validate_sse_limit("sse.max_line_bytes", self.sse_max_line_bytes)?;
        Self::validate_sse_limit("sse.max_frame_bytes", self.sse_max_frame_bytes)?;
        Ok(())
    }
}
