/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::result_large_err)]
// Integration tests favor readability of scenario setup; partial Default mutation and
// direct `HttpError` assertions are intentional.

//! # qubit-http integration tests
//!
//! Submodules mirror `src/` layout and are declared through ordinary module
//! files under each test directory.

mod client;
mod common;
mod error;
mod factory;
mod http_client;
mod options;
mod proxy;
mod request;
mod response;
mod retry_hint;
mod sanitize;
mod sse;
