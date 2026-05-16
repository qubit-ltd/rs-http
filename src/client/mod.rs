/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! HTTP client module root.

mod http_client;
mod http_client_factory;
pub mod http_logger;

pub use http_client::HttpClient;
pub use http_client_factory::HttpClientFactory;
