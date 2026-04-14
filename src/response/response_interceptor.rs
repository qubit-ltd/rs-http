/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Response interceptor abstraction for successful HTTP responses.

use qubit_function::ArcMutatingFunction;

use super::HttpResponseMeta;
use crate::HttpResult;

/// Response interceptor function used to inspect/mutate response metadata
/// (`status`, `headers`, `url`) before the response is returned to callers.
///
/// Returning `Err` short-circuits execution for the current attempt.
pub type ResponseInterceptor = ArcMutatingFunction<HttpResponseMeta, HttpResult<()>>;
