/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! # HTTP Client Options
//!
//! Defines all client-side configuration used by `qubit-http`.
//!
//! The configuration key layout for `from_config` is documented in the
//! `from_config_helpers` module (see `from_config_helpers.rs`).
//!

mod from_config_helpers;
mod http_client_options;
mod http_config_error;
mod http_config_error_kind;
mod http_logging_options;
mod http_retry_method_policy;
mod http_retry_options;
mod http_timeout_options;
mod proxy_options;
mod proxy_type;

pub use http_client_options::HttpClientOptions;
pub use http_config_error::HttpConfigError;
pub use http_config_error_kind::HttpConfigErrorKind;
pub use http_logging_options::HttpLoggingOptions;
pub use http_retry_method_policy::HttpRetryMethodPolicy;
pub use http_retry_options::HttpRetryOptions;
pub use http_timeout_options::HttpTimeoutOptions;
pub use proxy_options::ProxyOptions;
pub use proxy_type::ProxyType;
