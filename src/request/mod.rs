/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! # HTTP Request Types
//!
//! Defines request body, request object, request builder, and header injector.
//!

mod async_http_header_injector;
mod header_injector;
mod header_parser;
mod http_request;
mod http_request_body;
mod http_request_body_byte_stream;
mod http_request_builder;
mod http_request_interceptors;
mod http_request_retry_override;
mod http_request_streaming_body;

pub use async_http_header_injector::AsyncHttpHeaderInjector;
pub use header_injector::HttpHeaderInjector;
pub(crate) use header_parser::parse_header;
pub use http_request::HttpRequest;
pub use http_request_body::HttpRequestBody;
pub use http_request_body_byte_stream::HttpRequestBodyByteStream;
pub use http_request_builder::HttpRequestBuilder;
pub use http_request_interceptors::{
    HttpRequestInterceptor,
    HttpRequestInterceptors,
};
pub use http_request_retry_override::HttpRequestRetryOverride;
pub use http_request_streaming_body::HttpRequestStreamingBody;
