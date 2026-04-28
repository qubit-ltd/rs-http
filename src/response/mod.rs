/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! HTTP response types, streaming body aliases, and response interceptors.

mod http_byte_stream;
mod http_response;
mod http_response_interceptors;
mod http_response_meta;
mod http_response_options;

pub use http_byte_stream::HttpByteStream;
pub use http_response::HttpResponse;
pub use http_response_interceptors::{HttpResponseInterceptor, HttpResponseInterceptors};
pub use http_response_meta::HttpResponseMeta;
pub(crate) use http_response_options::HttpResponseOptions;

#[cfg(coverage)]
#[doc(hidden)]
pub(crate) use http_response::coverage_exercise_response_preview_paths;
