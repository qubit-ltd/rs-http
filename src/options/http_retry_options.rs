/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::Duration;

use http::StatusCode;
use qubit_config::{ConfigReader, ConfigResult};
use qubit_retry::{
    AttemptContext, Delay, Jitter, RetryDecision, RetryOptions,
};

use super::http_retry_method_policy::HttpRetryMethodPolicy;
use super::HttpConfigError;
use crate::{HttpError, HttpErrorKind, HttpRequest, HttpResult};

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
    /// Optional retryable status-code allowlist.
    ///
    /// When set, only listed statuses are retryable for status errors.
    pub retry_status_codes: Option<Vec<StatusCode>>,
    /// Optional retryable error-kind allowlist for non-status failures.
    ///
    /// When set, only listed kinds are retryable for non-status errors.
    pub retry_error_kinds: Option<Vec<HttpErrorKind>>,
}

/// Returns whether `status` is retryable for the given optional allowlist.
///
/// When `retry_status_codes` is `None`, uses [`default_retryable_status`].
fn is_retryable_status(status: StatusCode, retry_status_codes: Option<&[StatusCode]>) -> bool {
    if let Some(status_codes) = retry_status_codes {
        status_codes.contains(&status)
    } else {
        default_retryable_status(status)
    }
}

/// Returns whether `kind` is retryable for the given optional allowlist.
///
/// When `retry_error_kinds` is `None`, uses [`default_retryable_error_kind`].
fn is_retryable_error_kind(kind: HttpErrorKind, retry_error_kinds: Option<&[HttpErrorKind]>) -> bool {
    if let Some(error_kinds) = retry_error_kinds {
        error_kinds.contains(&kind)
    } else {
        default_retryable_error_kind(kind)
    }
}

impl HttpRetryOptions {
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
        if let Some(status_codes) = raw.status_codes.as_ref() {
            opts.retry_status_codes = Some(parse_retry_status_codes(status_codes)?);
        }
        if let Some(error_kinds) = raw.error_kinds.as_ref() {
            opts.retry_error_kinds = Some(parse_retry_error_kinds(error_kinds)?);
        }

        if let Some(delay_strategy) = raw.delay_strategy.as_ref() {
            opts.delay_strategy = parse_retry_delay_strategy(delay_strategy, &raw)?;
        }

        opts.validate()?;
        Ok(opts)
    }

    /// Reads retry options from a `ConfigReader`.
    ///
    /// # Parameters
    /// - `config`: Configuration reader whose keys are relative to the retry
    ///   configuration prefix.
    ///
    /// # Returns
    /// Parsed retry options or [`HttpConfigError`].
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
            status_codes: config.get_optional_string_list("status_codes")?,
            error_kinds: config.get_optional_string_list("error_kinds")?,
        })
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

    /// Returns whether retry should run for `request` under this policy.
    ///
    /// # Parameters
    /// - `request`: Request whose method is checked against retry policy.
    ///
    /// # Returns
    /// `true` when retry is enabled, `max_attempts` is greater than one, and
    /// the request method is allowed by [`Self::method_policy`].
    pub fn should_retry(&self, request: &HttpRequest) -> bool {
        self.max_attempts > 1 && self.allows_method(request.method())
    }

    /// Resolves request-level retry override against this retry policy.
    ///
    /// # Parameters
    /// - `request`: Request whose retry override is applied.
    ///
    /// # Returns
    /// Effective retry options for this request.
    pub fn resolve(&self, request: &HttpRequest) -> Self {
        let mut options = self.clone();
        options.enabled = request.retry_override().resolve_enabled(options.enabled);
        options.method_policy = request
            .retry_override()
            .resolve_method_policy(options.method_policy);
        options
    }

    /// Returns whether a status code is retryable under current retry policy.
    ///
    /// # Parameters
    /// - `status`: HTTP status code from the failure response.
    ///
    /// # Returns
    /// `true` if status should be retried.
    pub fn is_retryable_status(&self, status: StatusCode) -> bool {
        is_retryable_status(status, self.retry_status_codes.as_deref())
    }

    /// Returns whether a non-status error kind is retryable under current retry
    /// policy.
    ///
    /// # Parameters
    /// - `kind`: Error kind to evaluate.
    ///
    /// # Returns
    /// `true` if kind should be retried.
    pub fn is_retryable_error_kind(&self, kind: HttpErrorKind) -> bool {
        is_retryable_error_kind(kind, self.retry_error_kinds.as_deref())
    }

    /// Converts these options into [`RetryOptions`] for the built-in retry executor.
    ///
    /// # Errors
    /// Returns [`HttpError`] when executor limits or delay/jitter settings are invalid.
    pub fn to_executor_options(&self) -> HttpResult<RetryOptions> {
        RetryOptions::new(
            self.max_attempts,
            self.max_duration,
            self.delay_strategy.clone(),
            Jitter::factor(self.jitter_factor),
        )
        .map_err(|error| HttpError::other(format!("Invalid HTTP retry options: {error}")))
    }

    /// Returns an error classifier for [`qubit_retry::RetryExecutor::builder`] /
    /// [`qubit_retry::RetryExecutorBuilder::classify_error`].
    ///
    /// The closure captures clones of the status and error-kind allowlists only
    /// and delegates to [`is_retryable_status`] and [`is_retryable_error_kind`].
    pub fn to_executor_error_classifier(
        &self,
    ) -> impl Fn(&HttpError, &AttemptContext) -> RetryDecision + Send + Sync + 'static {
        let retry_status_codes = self.retry_status_codes.clone();
        let retry_error_kinds = self.retry_error_kinds.clone();
        move |error: &HttpError, _context: &AttemptContext| {
            let retryable = if error.kind == HttpErrorKind::Status {
                error.status.is_some_and(|status| {
                    is_retryable_status(status, retry_status_codes.as_deref())
                })
            } else {
                is_retryable_error_kind(error.kind, retry_error_kinds.as_deref())
            };
            if retryable {
                RetryDecision::Retry
            } else {
                RetryDecision::Abort
            }
        }
    }
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
            retry_status_codes: None,
            retry_error_kinds: None,
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
    status_codes: Option<Vec<String>>,
    error_kinds: Option<Vec<String>>,
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

/// Parses retry status-code list from config string values.
///
/// # Parameters
/// - `values`: Status-code strings from config.
///
/// # Returns
/// Normalized unique status-code list in ascending order.
///
/// # Errors
/// Returns [`HttpConfigError`] when any entry is blank or not a valid HTTP
/// status code.
fn parse_retry_status_codes(values: &[String]) -> Result<Vec<StatusCode>, HttpConfigError> {
    let mut result = Vec::<StatusCode>::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(HttpConfigError::invalid_value(
                "status_codes",
                "Retry status_codes cannot contain blank values",
            ));
        }
        let raw_code = trimmed.parse::<u16>().map_err(|error| {
            HttpConfigError::invalid_value(
                "status_codes",
                format!("Invalid retry status code '{trimmed}': {error}"),
            )
        })?;
        if !(100..=599).contains(&raw_code) {
            return Err(HttpConfigError::invalid_value(
                "status_codes",
                format!("Retry status code must be in range 100..=599, got {raw_code}"),
            ));
        }
        let status = StatusCode::from_u16(raw_code)
            .expect("retry status code range is pre-validated to 100..=599");
        if !result.contains(&status) {
            result.push(status);
        }
    }
    result.sort_by_key(|status| status.as_u16());
    Ok(result)
}

/// Parses retry error-kind list from config string values.
///
/// # Parameters
/// - `values`: Error-kind strings from config.
///
/// # Returns
/// Normalized unique error-kind list.
///
/// # Errors
/// Returns [`HttpConfigError`] when any entry is blank or unsupported.
fn parse_retry_error_kinds(values: &[String]) -> Result<Vec<HttpErrorKind>, HttpConfigError> {
    let mut result = Vec::<HttpErrorKind>::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(HttpConfigError::invalid_value(
                "error_kinds",
                "Retry error_kinds cannot contain blank values",
            ));
        }
        let kind = parse_retry_error_kind(trimmed).ok_or_else(|| {
            HttpConfigError::invalid_value(
                "error_kinds",
                format!("Unsupported retry error kind: {trimmed}"),
            )
        })?;
        if !result.contains(&kind) {
            result.push(kind);
        }
    }
    Ok(result)
}

/// Parses one retry error-kind token.
///
/// # Parameters
/// - `value`: Config token for one error kind.
///
/// # Returns
/// Parsed error kind, or `None` when unsupported.
fn parse_retry_error_kind(value: &str) -> Option<HttpErrorKind> {
    let normalized = value.trim().to_ascii_uppercase().replace('-', "_");
    match normalized.as_str() {
        "INVALID_URL" => Some(HttpErrorKind::InvalidUrl),
        "BUILD_CLIENT" => Some(HttpErrorKind::BuildClient),
        "PROXY_CONFIG" => Some(HttpErrorKind::ProxyConfig),
        "CONNECT_TIMEOUT" => Some(HttpErrorKind::ConnectTimeout),
        "READ_TIMEOUT" => Some(HttpErrorKind::ReadTimeout),
        "WRITE_TIMEOUT" => Some(HttpErrorKind::WriteTimeout),
        "REQUEST_TIMEOUT" => Some(HttpErrorKind::RequestTimeout),
        "TRANSPORT" => Some(HttpErrorKind::Transport),
        "STATUS" => Some(HttpErrorKind::Status),
        "DECODE" => Some(HttpErrorKind::Decode),
        "SSE_PROTOCOL" => Some(HttpErrorKind::SseProtocol),
        "SSE_DECODE" => Some(HttpErrorKind::SseDecode),
        "CANCELLED" => Some(HttpErrorKind::Cancelled),
        "OTHER" => Some(HttpErrorKind::Other),
        _ => None,
    }
}

/// Returns default retryable status policy when no explicit status allowlist is
/// configured.
///
/// # Parameters
/// - `status`: HTTP status code to evaluate.
///
/// # Returns
/// `true` for `429` and `5xx`, otherwise `false`.
fn default_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// Returns default retryable non-status error-kind policy when no explicit
/// error-kind allowlist is configured.
///
/// # Parameters
/// - `kind`: Error kind to evaluate.
///
/// # Returns
/// `true` for timeout and transport failures, otherwise `false`.
fn default_retryable_error_kind(kind: HttpErrorKind) -> bool {
    matches!(
        kind,
        HttpErrorKind::ConnectTimeout
            | HttpErrorKind::ReadTimeout
            | HttpErrorKind::WriteTimeout
            | HttpErrorKind::RequestTimeout
            | HttpErrorKind::Transport
    )
}
