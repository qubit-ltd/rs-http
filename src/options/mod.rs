/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # HTTP Client Options
//!
//! Defines all client-side configuration used by `qubit-http`.
//!
//! The configuration key layout for `from_config` is documented in the
//! `from_config_helpers` module (see `from_config_helpers.rs`).
//!
//! # Author
//!
//! Haixing Hu

mod from_config_helpers;
mod http_client_options;
mod http_config_error;
mod http_config_error_kind;
mod logging_options;
mod proxy_options;
mod proxy_type;
mod sensitive_headers;
mod timeout_options;

pub use http_client_options::HttpClientOptions;
pub use http_config_error::HttpConfigError;
pub use http_config_error_kind::HttpConfigErrorKind;
pub use logging_options::HttpLoggingOptions;
pub use proxy_options::ProxyOptions;
pub use proxy_type::ProxyType;
pub use sensitive_headers::SensitiveHeaders;
pub use timeout_options::TimeoutOptions;
