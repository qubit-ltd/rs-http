/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Header injector abstraction for outgoing requests.

use http::HeaderMap;
use qubit_function::ArcMutatingFunction;

use crate::HttpResult;

/// Header injector function used to mutate outgoing request headers.
///
/// This alias keeps the HTTP-domain name while directly reusing
/// [`ArcMutatingFunction`], so callers can construct injectors with
/// `HeaderInjector::new(...)`.
pub type HeaderInjector = ArcMutatingFunction<HeaderMap, HttpResult<()>>;
