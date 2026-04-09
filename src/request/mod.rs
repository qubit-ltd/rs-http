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

mod header_injector;
mod http_request;
mod http_request_body;
mod http_request_builder;

pub use header_injector::HeaderInjector;
pub use http_request::HttpRequest;
pub use http_request_body::HttpRequestBody;
pub use http_request_builder::HttpRequestBuilder;
