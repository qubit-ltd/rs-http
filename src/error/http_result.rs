/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! [`Result`] alias for HTTP operations.

use super::http_error::HttpError;

/// [`Result`] alias for HTTP operations: success value `T` or [`HttpError`].
pub type HttpResult<T> = Result<T, HttpError>;
