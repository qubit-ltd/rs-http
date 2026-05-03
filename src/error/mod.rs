/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! # HTTP Error Types
//!
//! Defines the unified error model used by this crate.
//!

pub(crate) mod backend_error_mapper;
mod http_error;
mod http_error_kind;
mod http_result;
mod reqwest_error_phase;
mod retry_hint;

pub use http_error::HttpError;
pub use http_error_kind::HttpErrorKind;
pub use http_result::HttpResult;
pub(crate) use reqwest_error_phase::ReqwestErrorPhase;
pub use retry_hint::RetryHint;
