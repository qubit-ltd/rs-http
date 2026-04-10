/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # Test Utilities
//!
//! Shared helpers for integration tests.

mod one_shot_server;
mod proxy_server;
mod tracing_capture;

pub use one_shot_server::{spawn_one_shot_server, ResponseChunk, ResponsePlan};
pub use proxy_server::{spawn_simple_proxy_server, ProxyBehavior};
pub use tracing_capture::capture_trace_logs;
