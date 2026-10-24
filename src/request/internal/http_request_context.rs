// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Client-derived context captured by one HTTP request.

use http::HeaderMap;
use qubit_redact::Redactor;
use url::Url;

use crate::AsyncHttpHeaderInjector;
use crate::HttpHeaderInjector;
use crate::HttpOriginPolicy;

/// Request context captured from the originating client.
#[derive(Debug, Clone)]
pub(in crate::request) struct HttpRequestContext {
    /// Base URL copied from client options, used to resolve relative `path`.
    pub(in crate::request) base_url: Option<Url>,
    /// Policy checked against the resolved URL before sending the request.
    pub(in crate::request) origin_policy: HttpOriginPolicy,
    /// Whether resolved URLs must avoid IPv6 literal hosts.
    pub(in crate::request) ipv4_only: bool,
    /// Client default headers snapshot captured when this request builder was
    /// created.
    pub(in crate::request) default_headers: HeaderMap,
    /// Client sync header injectors snapshot captured when this request
    /// builder was created.
    pub(in crate::request) injectors: Vec<HttpHeaderInjector>,
    /// Client async header injectors snapshot captured when this request
    /// builder was created.
    pub(in crate::request) async_injectors: Vec<AsyncHttpHeaderInjector>,
    /// Shared log redactor snapshot captured when this request builder was
    /// created.
    pub(in crate::request) log_redactor: Redactor,
}
