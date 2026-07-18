// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![allow(clippy::result_large_err)]
// Keep `HttpError` rich (method/url/status/source) for diagnostics and retry
// decisions across the crate's public APIs.

//! # Qubit HTTP
//!
//! A general-purpose HTTP infrastructure module for Rust services.
//!
//! This crate provides:
//! - Unified HTTP client options and factory abstractions
//! - Loading those options from [`qubit_config::ConfigReader`] (`from_config` /
//!   factory `create_from_config`)
//! - Consistent request/response/stream APIs
//! - Secure and configurable logging with URL, header, and body sanitization
//! - Built-in SSE decoding utilities in [`sse`]
//! - Unified error model and retry hints

mod client;
pub mod constants;
mod content_type;
mod error;
mod options;
mod request;
mod response;
mod sanitize;
pub mod sse;

pub use client::http_logger::HttpLogger;
pub use client::HttpClient;
pub use client::HttpClientFactory;
pub use error::{
    HttpError,
    HttpErrorKind,
    HttpResult,
    RetryHint,
};
pub use options::{
    HttpClientOptions,
    HttpConfigError,
    HttpConfigErrorKind,
    HttpLoggingOptions,
    HttpRetryMethodPolicy,
    HttpRetryOptions,
    HttpTimeoutOptions,
    ProxyOptions,
    ProxyType,
};
pub use qubit_retry::{
    RetryDelay,
    RetryJitter,
    RetryOptions,
};
pub use request::{
    AsyncHttpHeaderInjector,
    HttpHeaderInjector,
    HttpRequest,
    HttpRequestBody,
    HttpRequestBodyByteStream,
    HttpRequestBuilder,
    HttpRequestInterceptor,
    HttpRequestInterceptors,
    HttpRequestRetryOverride,
    HttpRequestStreamingBody,
};
pub use response::{
    HttpByteStream,
    HttpResponse,
    HttpResponseInterceptor,
    HttpResponseInterceptorContext,
    HttpResponseInterceptors,
    HttpResponseMeta,
};
pub use sanitize::{
    LogSanitizePolicy,
    LogSanitizer,
};
pub use tokio_util::sync::CancellationToken;
