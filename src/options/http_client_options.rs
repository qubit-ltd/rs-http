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
use crate::request::parse_header;
use crate::HttpResult;

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
}

impl Default for HttpClientOptions {
    /// Default: no base URL, empty headers, default timeouts/proxy/logging,
    /// default sensitive headers, IPv4-only off.
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
        }
    }
}

/// Top-level scalar keys read before nested sections and `default_headers` iteration.
struct HttpClientRootConfigInput {
    base_url: Option<String>,
    ipv4_only: Option<bool>,
    sensitive_headers: Option<Vec<String>>,
}

impl HttpClientOptions {
    fn read_config<R: ConfigReader + ?Sized>(
        config: &R,
    ) -> ConfigResult<HttpClientRootConfigInput> {
        Ok(HttpClientRootConfigInput {
            base_url: config.get_optional_string("base_url")?,
            ipv4_only: config.get_optional("ipv4_only")?,
            sensitive_headers: config.get_optional_string_list("sensitive_headers")?,
        })
    }

    fn parse_base_url(base_url: &str) -> Result<Url, HttpConfigError> {
        Url::parse(base_url).map_err(|error| {
            HttpConfigError::invalid_value("base_url", format!("Invalid URL: {error}"))
        })
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
    pub fn from_config<R: ConfigReader + ?Sized>(config: &R) -> Result<Self, HttpConfigError> {
        let mut opts = HttpClientOptions::default();

        let root = Self::read_config(config).map_err(HttpConfigError::from)?;

        if let Some(s) = root.base_url {
            opts.set_base_url(&s)?;
        }

        if let Some(v) = root.ipv4_only {
            opts.ipv4_only = v;
        }

        // timeouts
        if config.contains_prefix("timeouts") {
            opts.timeouts = TimeoutOptions::from_config(&config.prefix_view("timeouts"))
                .map_err(|e| e.prepend_path_prefix("timeouts"))?;
        }

        // proxy
        if config.contains_prefix("proxy") {
            opts.proxy = ProxyOptions::from_config(&config.prefix_view("proxy"))
                .map_err(|e| e.prepend_path_prefix("proxy"))?;
        }

        // logging
        if config.contains_prefix("logging") {
            opts.logging = HttpLoggingOptions::from_config(&config.prefix_view("logging"))
                .map_err(|e| e.prepend_path_prefix("logging"))?;
        }

        if config.contains_prefix("retry") {
            opts.retry = HttpRetryOptions::from_config(&config.prefix_view("retry"))
                .map_err(|e| e.prepend_path_prefix("retry"))?;
        }

        // default_headers – sub-key form: default_headers.<name> = <value>
        let headers_prefix = "default_headers";
        let full_headers_prefix = "default_headers.";
        let mut header_map: HashMap<String, String> = HashMap::new();
        for (k, _) in config.iter_prefix(full_headers_prefix) {
            let header_name = &k[full_headers_prefix.len()..];
            let value = config
                .get_string(k)
                .map_err(|e| HttpConfigError::config_error(k, e.to_string()))?;
            header_map.insert(header_name.to_string(), value);
        }
        // Also support JSON map form stored at the exact key `default_headers`.
        if header_map.is_empty() {
            if let Some(json_str) = config
                .get_optional_string(headers_prefix)
                .map_err(HttpConfigError::from)?
            {
                let parsed: HashMap<String, String> =
                    serde_json::from_str(&json_str).map_err(|e| {
                        HttpConfigError::type_error(
                            headers_prefix,
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

    /// Runs [`ProxyOptions::validate`] and [`HttpLoggingOptions::validate`].
    ///
    /// # Returns
    /// `Ok(())` or the first sub-validator error.
    pub fn validate(&self) -> Result<(), HttpConfigError> {
        self.proxy.validate()?;
        self.logging.validate()?;
        self.retry
            .validate()
            .map_err(|e| e.prepend_path_prefix("retry"))?;
        Ok(())
    }
}
