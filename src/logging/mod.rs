/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! # HTTP Logging
//!
//! Internal logging helpers for request/response traces.
//!
//! # Author
//!
//! Haixing Hu

mod masker;
mod policy;

pub use masker::mask_header_value;
pub use policy::{log_request, log_response, log_stream_response_headers};
