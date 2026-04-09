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

use super::HttpConfigError;

use super::from_config_helpers::hashmap_to_headermap;
use super::logging_options::HttpLoggingOptions;
use super::proxy_options::ProxyOptions;
use super::sensitive_headers::SensitiveHeaders;
use super::timeout_options::TimeoutOptions;

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

    /// Same as [`HttpClientOptions::default`].
    ///
    /// # Returns
    /// Fresh options with crate defaults.
    pub fn new() -> Self {
        Self::default()
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
            let url = Url::parse(&s).map_err(|e| {
                HttpConfigError::invalid_value("base_url", format!("Invalid URL: {e}"))
            })?;
            opts.base_url = Some(url);
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
        Ok(())
    }
}
