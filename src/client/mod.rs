/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! HTTP client module root.

mod error_mapper;
mod http_client;
pub mod http_logger;
mod request_pipeline;
mod retry_controller;
mod sse_reconnect;

pub use http_client::HttpClient;
