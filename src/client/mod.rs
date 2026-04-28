/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! HTTP client module root.

mod http_client;
mod http_client_factory;
pub mod http_logger;

pub use http_client::HttpClient;
pub use http_client_factory::HttpClientFactory;

#[cfg(coverage)]
#[doc(hidden)]
pub(crate) use http_client_factory::coverage_exercise_factory_paths;
#[cfg(coverage)]
#[doc(hidden)]
pub(crate) use http_logger::coverage_exercise_request_log_url_fallback;
