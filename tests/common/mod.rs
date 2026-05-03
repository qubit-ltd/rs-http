/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! # Test Utilities
//!
//! Shared helpers for integration tests.

mod one_shot_server;
mod proxy_server;
mod tracing_capture;

pub use one_shot_server::{
    spawn_multi_shot_server, spawn_one_shot_server, ResponseChunk, ResponsePlan,
};
pub use proxy_server::{spawn_simple_proxy_server, ProxyBehavior};
pub use tracing_capture::capture_trace_logs;
