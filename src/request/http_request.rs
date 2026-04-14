/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Immutable HTTP request object.

use std::time::Duration;

use http::{HeaderMap, Method};
use tokio_util::sync::CancellationToken;
use url::Host;
use url::Url;

use crate::HttpError;

use super::http_request_body::HttpRequestBody;
use super::http_request_retry_override::HttpRequestRetryOverride;

/// Immutable snapshot of a single HTTP call produced by [`crate::HttpRequestBuilder`].
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// HTTP method (GET, POST, …).
    pub method: Method,
    /// Absolute URL string, or path joined with client `base_url` when not parseable as URL.
    pub path: String,
    /// Query string parameters as `(name, value)` pairs.
    pub query: Vec<(String, String)>,
    /// Headers added on top of client defaults and injector output.
    pub headers: HeaderMap,
    /// Serialized body variant.
    pub body: HttpRequestBody,
    /// Overrides client-wide request timeout when set; otherwise client default applies.
    pub request_timeout: Option<Duration>,
    /// Base URL copied from client options, used to resolve relative `path`.
    pub base_url: Option<Url>,
    /// Whether resolved URLs must avoid IPv6 literal hosts.
    pub ipv4_only: bool,
    /// Optional cancellation token checked before send and during I/O phases.
    pub cancellation_token: Option<CancellationToken>,
    /// Per-request retry override (enable/disable/method-policy/Retry-After behavior).
    pub retry_override: HttpRequestRetryOverride,
}

impl HttpRequest {
    /// Resolves `self.path` as an absolute URL or joins it with `self.base_url` when relative.
    ///
    /// # Returns
    /// Resolved [`Url`] or [`HttpError::invalid_url`] if resolution/validation fails.
    pub(crate) fn resolve_url(&self) -> Result<Url, HttpError> {
        if let Ok(url) = Url::parse(&self.path) {
            self.validate_resolved_url_host(&url)?;
            return Ok(url);
        }

        let base = self.base_url.as_ref().ok_or_else(|| {
            HttpError::invalid_url(format!(
                "Cannot resolve relative path '{}' without base_url",
                self.path
            ))
        })?;

        let url = base.join(&self.path).map_err(|error| {
            HttpError::invalid_url(format!(
                "Failed to resolve path '{}' against base URL '{}': {}",
                self.path, base, error
            ))
        })?;
        self.validate_resolved_url_host(&url)?;
        Ok(url)
    }

    /// Rejects IPv6 literal hosts when `ipv4_only` is enabled.
    fn validate_resolved_url_host(&self, url: &Url) -> Result<(), HttpError> {
        if self.ipv4_only && matches!(url.host(), Some(Host::Ipv6(_))) {
            return Err(HttpError::invalid_url(format!(
                "IPv6 literal host is not allowed when ipv4_only=true: {}",
                url
            )));
        }
        Ok(())
    }

    /// Builds a cancelled error when this request's cancellation token exists and is already cancelled.
    ///
    /// # Parameters
    /// - `url`: Resolved request URL for error context.
    /// - `message`: Cancellation message.
    ///
    /// # Returns
    /// `Some(HttpError)` when cancelled, otherwise `None`.
    pub(crate) fn cancelled_error_if_needed(
        &self,
        url: &Url,
        message: &str,
    ) -> Option<HttpError> {
        if self
            .cancellation_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            Some(
                HttpError::cancelled(message.to_string())
                    .with_method(self.method.clone())
                    .with_url(url.clone()),
            )
        } else {
            None
        }
    }
}
