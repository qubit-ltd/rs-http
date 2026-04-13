/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
#![allow(clippy::result_large_err)]
// Keep `HttpError` rich (method/url/status/source) for diagnostics and retry decisions
// across the crate's public APIs.

//! # Qubit HTTP
//!
//! A general-purpose HTTP infrastructure module for Rust services.
//!
//! This crate provides:
//! - Unified HTTP client options and factory abstractions
//! - Loading those options from [`qubit_config::ConfigReader`] (`from_config` / factory `create_from_config`)
//! - Consistent request/response/stream APIs
//! - Secure and configurable logging with sensitive header masking
//! - Built-in SSE decoding utilities in [`sse`]
//! - Unified error model and retry hints
//!
//! # Author
//!
//! Haixing Hu

pub mod constants;
mod error;
mod http_byte_stream;
mod http_client;
mod http_client_factory;
mod http_logger;
mod http_response;
mod http_stream_response;
mod options;
mod request;
mod retry_hint;
pub mod sse;

pub use constants::DEFAULT_SENSITIVE_HEADER_NAMES;
pub use error::{HttpError, HttpErrorKind, HttpResult};
pub use http_byte_stream::HttpByteStream;
pub use http_client::HttpClient;
pub use http_client_factory::HttpClientFactory;
pub use http_logger::HttpLogger;
pub use http_response::HttpResponse;
pub use http_stream_response::HttpStreamResponse;
pub use options::{
    HttpClientOptions, HttpConfigError, HttpConfigErrorKind, HttpLoggingOptions,
    HttpRetryMethodPolicy, HttpRetryOptions, ProxyOptions, ProxyType, SensitiveHeaders,
    TimeoutOptions,
};
pub use qubit_retry::{Delay, Jitter};
pub use request::{HeaderInjector, HttpRequest, HttpRequestBody, HttpRequestBuilder};
pub use retry_hint::RetryHint;
