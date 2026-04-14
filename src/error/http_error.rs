/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Unified [`HttpError`] type.

use std::error::Error;
use std::time::Duration;

use http::{Method, StatusCode};
use thiserror::Error;
use url::Url;

use crate::RetryHint;
use qubit_common::BoxError;

use super::HttpErrorKind;

/// Unified HTTP error type.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct HttpError {
    /// Error category.
    pub kind: HttpErrorKind,
    /// Optional HTTP method.
    pub method: Option<Method>,
    /// Optional request URL.
    pub url: Option<Url>,
    /// Optional response status code.
    pub status: Option<StatusCode>,
    /// Human-readable message.
    pub message: String,
    /// Optional preview of non-success response body.
    pub response_body_preview: Option<String>,
    /// Optional `Retry-After` duration parsed from a non-success response.
    pub retry_after: Option<Duration>,
    /// Optional source error.
    #[source]
    pub source: Option<BoxError>,
}

impl HttpError {
    /// Creates an error with kind and message; other fields are unset until chained.
    ///
    /// # Parameters
    /// - `kind`: Classification for retry logic and handling.
    /// - `message`: Human-readable description.
    ///
    /// # Returns
    /// New [`HttpError`].
    pub fn new(kind: HttpErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            method: None,
            url: None,
            status: None,
            message: message.into(),
            response_body_preview: None,
            retry_after: None,
            source: None,
        }
    }

    /// Attaches the HTTP method for diagnostics.
    ///
    /// # Parameters
    /// - `method`: Request method associated with the failure.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn with_method(mut self, method: Method) -> Self {
        self.method = Some(method);
        self
    }

    /// Attaches the request URL for diagnostics.
    ///
    /// # Parameters
    /// - `url`: Request URL associated with the failure.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn with_url(mut self, url: Url) -> Self {
        self.url = Some(url);
        self
    }

    /// Attaches an HTTP status code (e.g. for [`HttpErrorKind::Status`]).
    ///
    /// # Parameters
    /// - `status`: Response status code.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = Some(status);
        self
    }

    /// Wraps an underlying error as the [`HttpError::source`] chain.
    ///
    /// # Parameters
    /// - `source`: Root cause (`Send + Sync + 'static`).
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn with_source<E>(mut self, source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        self.source = Some(Box::new(source));
        self
    }

    /// Attaches a preview of the non-success response body.
    ///
    /// # Parameters
    /// - `preview`: Truncated or summarized response body text.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn with_response_body_preview(mut self, preview: impl Into<String>) -> Self {
        self.response_body_preview = Some(preview.into());
        self
    }

    /// Attaches parsed `Retry-After` duration from a non-success response.
    ///
    /// # Parameters
    /// - `retry_after`: Parsed retry delay.
    ///
    /// # Returns
    /// `self` for chaining.
    pub fn with_retry_after(mut self, retry_after: Duration) -> Self {
        self.retry_after = Some(retry_after);
        self
    }

    /// Builds [`HttpErrorKind::InvalidUrl`].
    ///
    /// # Parameters
    /// - `message`: Why the URL is invalid or cannot be resolved.
    ///
    /// # Returns
    /// New [`HttpError`].
    pub fn invalid_url(message: impl Into<String>) -> Self {
        Self::new(HttpErrorKind::InvalidUrl, message)
    }

    /// Builds [`HttpErrorKind::BuildClient`] (e.g. reqwest builder failure).
    ///
    /// # Parameters
    /// - `message`: Build failure description.
    ///
    /// # Returns
    /// New [`HttpError`].
    pub fn build_client(message: impl Into<String>) -> Self {
        Self::new(HttpErrorKind::BuildClient, message)
    }

    /// Builds [`HttpErrorKind::ProxyConfig`].
    ///
    /// # Parameters
    /// - `message`: Invalid proxy settings explanation.
    ///
    /// # Returns
    /// New [`HttpError`].
    pub fn proxy_config(message: impl Into<String>) -> Self {
        Self::new(HttpErrorKind::ProxyConfig, message)
    }

    /// Builds [`HttpErrorKind::ConnectTimeout`].
    ///
    /// # Parameters
    /// - `message`: Timeout context.
    ///
    /// # Returns
    /// New [`HttpError`].
    pub fn connect_timeout(message: impl Into<String>) -> Self {
        Self::new(HttpErrorKind::ConnectTimeout, message)
    }

    /// Builds [`HttpErrorKind::ReadTimeout`].
    ///
    /// # Parameters
    /// - `message`: Timeout context.
    ///
    /// # Returns
    /// New [`HttpError`].
    pub fn read_timeout(message: impl Into<String>) -> Self {
        Self::new(HttpErrorKind::ReadTimeout, message)
    }

    /// Builds [`HttpErrorKind::WriteTimeout`].
    ///
    /// # Parameters
    /// - `message`: Timeout context.
    ///
    /// # Returns
    /// New [`HttpError`].
    pub fn write_timeout(message: impl Into<String>) -> Self {
        Self::new(HttpErrorKind::WriteTimeout, message)
    }

    /// Builds [`HttpErrorKind::RequestTimeout`].
    ///
    /// # Parameters
    /// - `message`: Timeout context for the whole request deadline.
    ///
    /// # Returns
    /// New [`HttpError`].
    pub fn request_timeout(message: impl Into<String>) -> Self {
        Self::new(HttpErrorKind::RequestTimeout, message)
    }

    /// Builds [`HttpErrorKind::Transport`].
    ///
    /// # Parameters
    /// - `message`: Low-level I/O or network failure description.
    ///
    /// # Returns
    /// New [`HttpError`].
    pub fn transport(message: impl Into<String>) -> Self {
        Self::new(HttpErrorKind::Transport, message)
    }

    /// Builds [`HttpErrorKind::Status`] with the given status pre-filled.
    ///
    /// # Parameters
    /// - `status`: HTTP status from the response.
    /// - `message`: Additional context.
    ///
    /// # Returns
    /// New [`HttpError`] with [`HttpError::status`] set.
    pub fn status(status: StatusCode, message: impl Into<String>) -> Self {
        Self::new(HttpErrorKind::Status, message).with_status(status)
    }

    /// Builds [`HttpErrorKind::Decode`] (body or payload decoding).
    ///
    /// # Parameters
    /// - `message`: Decode failure description.
    ///
    /// # Returns
    /// New [`HttpError`].
    pub fn decode(message: impl Into<String>) -> Self {
        Self::new(HttpErrorKind::Decode, message)
    }

    /// Builds [`HttpErrorKind::SseProtocol`] (framing, UTF-8, SSE line rules).
    ///
    /// # Parameters
    /// - `message`: Protocol violation description.
    ///
    /// # Returns
    /// New [`HttpError`].
    pub fn sse_protocol(message: impl Into<String>) -> Self {
        Self::new(HttpErrorKind::SseProtocol, message)
    }

    /// Builds [`HttpErrorKind::SseDecode`] (e.g. JSON in SSE data).
    ///
    /// # Parameters
    /// - `message`: Payload decode failure description.
    ///
    /// # Returns
    /// New [`HttpError`].
    pub fn sse_decode(message: impl Into<String>) -> Self {
        Self::new(HttpErrorKind::SseDecode, message)
    }

    /// Builds [`HttpErrorKind::Cancelled`].
    ///
    /// # Parameters
    /// - `message`: Why the operation was cancelled.
    ///
    /// # Returns
    /// New [`HttpError`].
    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::new(HttpErrorKind::Cancelled, message)
    }

    /// Builds [`HttpErrorKind::Other`].
    ///
    /// # Parameters
    /// - `message`: Catch-all description.
    ///
    /// # Returns
    /// New [`HttpError`].
    pub fn other(message: impl Into<String>) -> Self {
        Self::new(HttpErrorKind::Other, message)
    }

    /// Classifies this error for retry policies ([`RetryHint`]).
    ///
    /// # Returns
    /// [`RetryHint::Retryable`] for timeouts, transport errors, and some HTTP statuses; otherwise non-retryable.
    pub fn retry_hint(&self) -> RetryHint {
        match self.kind {
            HttpErrorKind::ConnectTimeout
            | HttpErrorKind::ReadTimeout
            | HttpErrorKind::WriteTimeout
            | HttpErrorKind::RequestTimeout
            | HttpErrorKind::Transport => RetryHint::Retryable,
            HttpErrorKind::Status => {
                if let Some(status) = self.status {
                    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                        RetryHint::Retryable
                    } else {
                        RetryHint::NonRetryable
                    }
                } else {
                    RetryHint::NonRetryable
                }
            }
            _ => RetryHint::NonRetryable,
        }
    }
}

impl From<std::io::Error> for HttpError {
    /// Maps [`std::io::Error`] to [`HttpError::transport`] with the I/O error as source.
    ///
    /// # Parameters
    /// - `error`: Underlying I/O error.
    ///
    /// # Returns
    /// Wrapped [`HttpError`].
    fn from(error: std::io::Error) -> Self {
        Self::transport(error.to_string()).with_source(error)
    }
}

impl From<reqwest::Error> for HttpError {
    /// Maps [`reqwest::Error`] to [`HttpErrorKind::BuildClient`] with chained source.
    ///
    /// # Parameters
    /// - `error`: Reqwest error to wrap.
    ///
    /// # Returns
    /// Wrapped [`HttpError`].
    fn from(error: reqwest::Error) -> Self {
        Self::build_client(format!("Failed to build reqwest client: {}", error)).with_source(error)
    }
}
