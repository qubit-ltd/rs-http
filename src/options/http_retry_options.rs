/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::Duration;

use qubit_config::{ConfigReader, ConfigResult};
use qubit_retry::Delay;

use super::http_retry_method_policy::HttpRetryMethodPolicy;
use super::HttpConfigError;

const DEFAULT_RETRY_MAX_ATTEMPTS: u32 = 3;
const DEFAULT_RETRY_INITIAL_DELAY: Duration = Duration::from_millis(200);
const DEFAULT_RETRY_MAX_DELAY: Duration = Duration::from_secs(5);
const DEFAULT_RETRY_MULTIPLIER: f64 = 2.0;
const DEFAULT_RETRY_JITTER_FACTOR: f64 = 0.1;

/// Retry settings for [`crate::HttpClient`].
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRetryOptions {
    /// Whether built-in retry is enabled.
    pub enabled: bool,
    /// Maximum number of attempts, including the first request.
    pub max_attempts: u32,
    /// Optional maximum total retry duration.
    pub max_duration: Option<Duration>,
    /// Delay strategy between attempts.
    pub delay_strategy: Delay,
    /// Jitter factor passed to the retry delay strategy.
    pub jitter_factor: f64,
    /// Method replay policy.
    pub method_policy: HttpRetryMethodPolicy,
}

impl Default for HttpRetryOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            max_attempts: DEFAULT_RETRY_MAX_ATTEMPTS,
            max_duration: None,
            delay_strategy: Delay::Exponential {
                initial: DEFAULT_RETRY_INITIAL_DELAY,
                max: DEFAULT_RETRY_MAX_DELAY,
                multiplier: DEFAULT_RETRY_MULTIPLIER,
            },
            jitter_factor: DEFAULT_RETRY_JITTER_FACTOR,
            method_policy: HttpRetryMethodPolicy::default(),
        }
    }
}

struct HttpRetryConfigInput {
    enabled: Option<bool>,
    max_attempts: Option<u32>,
    max_duration: Option<Duration>,
    delay_strategy: Option<String>,
    fixed_delay: Option<Duration>,
    random_min_delay: Option<Duration>,
    random_max_delay: Option<Duration>,
    backoff_initial_delay: Option<Duration>,
    backoff_max_delay: Option<Duration>,
    backoff_multiplier: Option<f64>,
    jitter_factor: Option<f64>,
    method_policy: Option<String>,
}

impl HttpRetryOptions {
    fn read_config<R>(config: &R) -> ConfigResult<HttpRetryConfigInput>
    where
        R: ConfigReader + ?Sized,
    {
        Ok(HttpRetryConfigInput {
            enabled: config.get_optional("enabled")?,
            max_attempts: config.get_optional("max_attempts")?,
            max_duration: config.get_optional("max_duration")?,
            delay_strategy: config.get_optional_string("delay_strategy")?,
            fixed_delay: config.get_optional("fixed_delay")?,
            random_min_delay: config.get_optional("random_min_delay")?,
            random_max_delay: config.get_optional("random_max_delay")?,
            backoff_initial_delay: config.get_optional("backoff_initial_delay")?,
            backoff_max_delay: config.get_optional("backoff_max_delay")?,
            backoff_multiplier: config.get_optional("backoff_multiplier")?,
            jitter_factor: config.get_optional("jitter_factor")?,
            method_policy: config.get_optional_string("method_policy")?,
        })
    }

    /// Creates default retry options.
    ///
    /// # Returns
    /// Fresh retry options with built-in retry disabled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates [`HttpRetryOptions`] from `config` using relative keys.
    ///
    /// # Parameters
    /// - `config`: Any [`ConfigReader`] scoped to the retry section.
    ///
    /// # Returns
    /// Parsed retry options or [`HttpConfigError`].
    pub fn from_config<R>(config: &R) -> Result<Self, HttpConfigError>
    where
        R: ConfigReader + ?Sized,
    {
        let raw = Self::read_config(config).map_err(HttpConfigError::from)?;
        let mut opts = Self::default();

        if let Some(enabled) = raw.enabled {
            opts.enabled = enabled;
        }
        if let Some(max_attempts) = raw.max_attempts {
            opts.max_attempts = max_attempts;
        }
        opts.max_duration = raw.max_duration;
        if let Some(jitter_factor) = raw.jitter_factor {
            opts.jitter_factor = jitter_factor;
        }
        if let Some(method_policy) = raw.method_policy.as_ref() {
            opts.method_policy = HttpRetryMethodPolicy::from_config_value(method_policy)?;
        }

        if let Some(delay_strategy) = raw.delay_strategy.as_ref() {
            opts.delay_strategy = parse_retry_delay_strategy(delay_strategy, &raw)?;
        }

        opts.validate()?;
        Ok(opts)
    }

    /// Runs retry option validation.
    ///
    /// # Returns
    /// `Ok(())` when values are usable, otherwise [`HttpConfigError`].
    pub fn validate(&self) -> Result<(), HttpConfigError> {
        if self.max_attempts == 0 {
            return Err(HttpConfigError::invalid_value(
                "max_attempts",
                "Retry max_attempts must be greater than 0",
            ));
        }
        if !(0.0..=1.0).contains(&self.jitter_factor) {
            return Err(HttpConfigError::invalid_value(
                "jitter_factor",
                "Retry jitter_factor must be between 0.0 and 1.0",
            ));
        }
        self.delay_strategy
            .validate()
            .map_err(|message| HttpConfigError::invalid_value("delay_strategy", message))?;
        Ok(())
    }

    /// Returns whether `method` is eligible for built-in retry.
    ///
    /// # Parameters
    /// - `method`: HTTP method to evaluate.
    ///
    /// # Returns
    /// `true` if retry is enabled and the method policy allows replay.
    pub fn allows_method(&self, method: &http::Method) -> bool {
        self.enabled && self.method_policy.allows_method(method)
    }
}

fn parse_retry_delay_strategy(
    value: &str,
    raw: &HttpRetryConfigInput,
) -> Result<Delay, HttpConfigError> {
    let normalized = value.trim().to_ascii_uppercase().replace('-', "_");
    match normalized.as_str() {
        "NONE" => Ok(Delay::None),
        "FIXED" => Ok(Delay::Fixed(
            raw.fixed_delay.unwrap_or(DEFAULT_RETRY_INITIAL_DELAY),
        )),
        "RANDOM" => Ok(Delay::Random {
            min: raw.random_min_delay.unwrap_or(DEFAULT_RETRY_INITIAL_DELAY),
            max: raw.random_max_delay.unwrap_or(DEFAULT_RETRY_MAX_DELAY),
        }),
        "EXPONENTIAL_BACKOFF" | "EXPONENTIAL" => Ok(Delay::Exponential {
            initial: raw
                .backoff_initial_delay
                .unwrap_or(DEFAULT_RETRY_INITIAL_DELAY),
            max: raw.backoff_max_delay.unwrap_or(DEFAULT_RETRY_MAX_DELAY),
            multiplier: raw.backoff_multiplier.unwrap_or(DEFAULT_RETRY_MULTIPLIER),
        }),
        _ => Err(HttpConfigError::invalid_value(
            "delay_strategy",
            format!("Unsupported retry delay strategy: {value}"),
        )),
    }
}
