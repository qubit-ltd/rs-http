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

mod client;
pub mod constants;
mod error;
mod options;
mod request;
mod response;
pub mod sse;

pub use client::http_logger::HttpLogger;
pub use client::HttpClient;
pub use client::HttpClientFactory;
pub use constants::DEFAULT_SENSITIVE_HEADER_NAMES;
pub use error::{HttpError, HttpErrorKind, HttpResult, RetryHint};
pub use options::{
    HttpClientOptions, HttpConfigError, HttpConfigErrorKind, HttpLoggingOptions,
    HttpRetryMethodPolicy, HttpRetryOptions, HttpTimeoutOptions, PendingHttpRetryAfterDelay,
    ProxyOptions, ProxyType, SensitiveHttpHeaders,
};
pub use qubit_retry::{RetryDelay, RetryJitter};
pub use request::{
    AsyncHttpHeaderInjector, HttpHeaderInjector, HttpRequest, HttpRequestBody,
    HttpRequestBodyByteStream, HttpRequestBuilder, HttpRequestInterceptor, HttpRequestInterceptors,
    HttpRequestRetryOverride, HttpRequestStreamingBody,
};
pub use response::{
    HttpByteStream, HttpResponse, HttpResponseInterceptor, HttpResponseInterceptors,
    HttpResponseMeta,
};
pub use tokio_util::sync::CancellationToken;
