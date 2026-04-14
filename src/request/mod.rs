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
mod http_request_builder;
mod http_request_retry_override;
mod request_interceptor;

pub(crate) fn parse_header(name: &str, value: &str) -> HttpResult<(HeaderName, HeaderValue)> {
    let header_name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|error| HttpError::other(format!("Invalid header name '{}': {}", name, error)))?;
    let header_value = HeaderValue::from_str(value).map_err(|error| {
        HttpError::other(format!("Invalid header value for '{}': {}", name, error))
    })?;
    Ok((header_name, header_value))
}

pub use async_header_injector::AsyncHeaderInjector;
pub use header_injector::HeaderInjector;
pub use http_request::HttpRequest;
pub use http_request_body::HttpRequestBody;
pub use http_request_builder::HttpRequestBuilder;
pub use http_request_retry_override::HttpRequestRetryOverride;
pub use request_interceptor::RequestInterceptor;
