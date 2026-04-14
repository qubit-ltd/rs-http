/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Request interceptor abstraction for outgoing HTTP requests.

use qubit_function::ArcMutatingFunction;

use super::HttpRequest;
use crate::HttpResult;

/// Request interceptor function used to mutate an outbound [`HttpRequest`]
/// before URL resolution, header merge, and network I/O.
///
/// Returning `Err` short-circuits execution for the current attempt.
pub type RequestInterceptor = ArcMutatingFunction<HttpRequest, HttpResult<()>>;
