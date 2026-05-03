/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! [`Result`] alias for HTTP operations.

use super::http_error::HttpError;

/// [`Result`] alias for HTTP operations: success value `T` or [`HttpError`].
pub type HttpResult<T> = Result<T, HttpError>;
