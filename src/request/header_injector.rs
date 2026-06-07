// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Header injector abstraction for outgoing requests.

use http::HeaderMap;
use qubit_function::ArcMutatingFunction;

use crate::HttpResult;

/// Header injector function used to mutate outgoing request headers.
///
/// This alias keeps the HTTP-domain name while directly reusing
/// [`ArcMutatingFunction`], so callers can construct injectors with
/// `HttpHeaderInjector::new(...)`.
pub type HttpHeaderInjector = ArcMutatingFunction<HeaderMap, HttpResult<()>>;
