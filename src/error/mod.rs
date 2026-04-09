/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # HTTP Error Types
//!
//! Defines the unified error model used by this crate.
//!
//! # Author
//!
//! Haixing Hu

mod http_error;
mod http_error_kind;
mod http_result;

pub use http_error::HttpError;
pub use http_error_kind::HttpErrorKind;
pub use http_result::HttpResult;
