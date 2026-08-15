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
//! - Secure and configurable logging with URL, header, and body redaction
//! - Built-in SSE decoding utilities in [`sse`]
//! - Unified error model and retry hints

mod client;
pub mod constants;
mod content_type;
mod error;
mod options;
mod redact;
mod request;
mod response;
pub mod sse;

pub use client::HttpClient;
pub use client::HttpClientFactory;
pub use client::http_logger::HttpLogger;
pub use error::HttpError;
pub use error::HttpErrorKind;
pub use error::HttpResult;
pub use error::RetryHint;
pub use options::HttpClientOptions;
pub use options::HttpConfigError;
pub use options::HttpConfigErrorKind;
pub use options::HttpLoggingOptions;
pub use options::HttpRetryMethodPolicy;
pub use options::HttpRetryOptions;
pub use options::HttpTimeoutOptions;
pub use options::ProxyOptions;
pub use options::ProxyType;
pub use qubit_retry::RetryCancellationToken;
pub use request::AsyncHttpHeaderInjector;
pub use request::HttpHeaderInjector;
pub use request::HttpRequest;
pub use request::HttpRequestBody;
pub use request::HttpRequestBodyByteStream;
pub use request::HttpRequestBuilder;
pub use request::HttpRequestInterceptor;
pub use request::HttpRequestInterceptors;
pub use request::HttpRequestRetryOverride;
pub use request::HttpRequestStreamingBody;
pub use response::HttpByteStream;
pub use response::HttpResponse;
pub use response::HttpResponseInterceptor;
pub use response::HttpResponseInterceptorContext;
pub use response::HttpResponseInterceptors;
pub use response::HttpResponseMeta;
