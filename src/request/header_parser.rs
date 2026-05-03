/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Header parsing helpers for request configuration paths.

use http::{HeaderName, HeaderValue};

use crate::{HttpError, HttpResult};

/// Parses a header name and value pair into typed HTTP header components.
///
/// # Parameters
/// - `name`: Header name text.
/// - `value`: Header value text.
///
/// # Returns
/// Parsed [`HeaderName`] and [`HeaderValue`] on success.
///
/// # Errors
/// Returns [`HttpError`] when `name` is not a valid HTTP header name or `value`
/// is not a valid HTTP header value.
pub(crate) fn parse_header(name: &str, value: &str) -> HttpResult<(HeaderName, HeaderValue)> {
    let header_name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|error| HttpError::other(format!("Invalid header name '{}': {}", name, error)))?;
    let header_value = HeaderValue::from_str(value).map_err(|error| {
        HttpError::other(format!("Invalid header value for '{}': {}", name, error))
    })?;
    Ok((header_name, header_value))
}
