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

pub use one_shot_server::{spawn_one_shot_server, ResponseChunk, ResponsePlan};
