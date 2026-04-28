/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # HTTP Request Types
//!
//! Defines request body, request object, request builder, and header injector.
//!
//! # Author
//!
//! Haixing Hu

use http::{HeaderName, HeaderValue};

use crate::{HttpError, HttpResult};

mod async_header_injector;
mod header_injector;
mod http_request;
mod http_request_body;
mod http_request_body_byte_stream;
mod http_request_builder;
mod http_request_retry_override;
mod http_request_streaming_body;
mod request_interceptor;

pub(crate) fn parse_header(name: &str, value: &str) -> HttpResult<(HeaderName, HeaderValue)> {
    let header_name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|error| HttpError::other(format!("Invalid header name '{}': {}", name, error)))?;
    let header_value = HeaderValue::from_str(value).map_err(|error| {
        HttpError::other(format!("Invalid header value for '{}': {}", name, error))
    })?;
    Ok((header_name, header_value))
}

pub use async_header_injector::AsyncHttpHeaderInjector;
pub use header_injector::HttpHeaderInjector;
pub use http_request::HttpRequest;
pub use http_request_body::HttpRequestBody;
pub use http_request_body_byte_stream::HttpRequestBodyByteStream;
pub use http_request_builder::HttpRequestBuilder;
pub use http_request_retry_override::HttpRequestRetryOverride;
pub use http_request_streaming_body::HttpRequestStreamingBody;
pub use request_interceptor::{HttpRequestInterceptor, HttpRequestInterceptors};

#[cfg(coverage)]
#[doc(hidden)]
pub(crate) use http_request::coverage_exercise_request_cache_paths;
