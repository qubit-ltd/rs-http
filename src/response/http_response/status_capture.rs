// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded non-success response capture and diagnostics.
use std::error::Error;
use std::sync::Arc;

use bytes::Bytes;
use http::header::CONTENT_TYPE;
use http::HeaderMap;
use http::HeaderValue;
use qubit_budget::ResourceBudget;
use qubit_redact::formats::http::BodyCapture;
use qubit_redact::Redactor;
use tokio::time::Instant;

use super::super::internal::http_error_body_capture::HttpErrorBodyCapture;
use super::super::internal::HttpResponseBodyState;
use super::HttpResponse;
use crate::HttpCancellationToken;
use crate::HttpError;
use crate::HttpResult;

impl HttpResponse {
    /// Returns `Ok(self)` for success statuses, otherwise maps a status error
    /// with `Retry-After` and response-body preview context.
    ///
    /// # Parameters
    /// - `message_prefix`: Context prepended to the status error message.
    ///
    /// # Returns
    /// This response when its status is successful.
    ///
    /// # Errors
    /// Returns a `Status` error for unsuccessful responses. Body capture
    /// failures and timeouts are retained in the capture diagnostics;
    /// cancellation returns `Cancelled`.
    pub(crate) async fn into_success_or_status_error(
        self,
        message_prefix: &str,
    ) -> HttpResult<Self> {
        let status = self.status();
        if status.is_success() {
            return Ok(self);
        }
        let retry_after = self.retry_after_hint();
        let method = self.meta.method().clone();
        let url = self.request_url().clone();
        let error_preview_limit = self.options.error_response_preview_limit;
        let error_body_headers = self.meta.headers().clone();
        let error_body_content_length = self.content_length_hint();
        let error_body_limit = self.options.error_response_body_limit;
        let log_redactor = self.log_redactor().clone();
        let capture = self
            .into_error_body_capture(error_preview_limit, error_body_limit)
            .await?;
        let redacted_url = log_redactor
            .redact_http_url(url.as_str())
            .into_text_or_marker("<redaction incomplete>");
        let message = format!(
            "{} with status {} for {} {}; response body preview: {}",
            message_prefix, status, method, redacted_url, capture.preview
        );
        let mut mapped = HttpError::from_status(status, message)
            .with_method(&method)
            .with_url(&url)
            .with_response_body_preview(capture.preview.clone())
            .with_log_redactor(log_redactor.clone());
        if let Some(retry_after) = retry_after {
            mapped = mapped.with_retry_after(retry_after);
        }
        mapped = mapped.with_status_response(crate::error::HttpStatusResponse::new(
            error_body_headers,
            capture.raw_body,
            capture.end,
            capture.error,
            error_body_content_length,
        ));
        Err(mapped)
    }

    /// Consumes this response and captures a bounded preview input and raw body
    /// prefix for a status error. Preview truncation is independent of raw
    /// capture completeness.
    ///
    /// # Parameters
    /// - `preview_limit`: Maximum raw prefix bytes supplied to preview
    ///   rendering; this is not a hard limit on the final rendered text length.
    /// - `raw_limit`: Maximum raw body prefix bytes retained for status
    ///   inspection.
    ///
    /// # Returns
    /// Captured body data, the redactor-rendered preview, the capture end
    /// reason, and an optional backend read error. Redactor policy and
    /// marker text govern the final displayed representation.
    ///
    /// # Errors
    /// Returns [`HttpErrorKind::Cancelled`](crate::HttpErrorKind::Cancelled)
    /// when the request cancellation token fires during capture.
    async fn into_error_body_capture(
        mut self,
        preview_limit: usize,
        raw_limit: usize,
    ) -> HttpResult<HttpErrorBodyCapture> {
        let limit = preview_limit.max(1);
        let state = std::mem::replace(&mut self.body, HttpResponseBodyState::StreamingTaken);
        let content_type = Self::content_type_header(self.meta.headers());
        let backend = match state {
            HttpResponseBodyState::Backend(backend) => backend,
            HttpResponseBodyState::Buffered(body) => {
                let raw_limit = raw_limit.max(1);
                let end = if body.len() > raw_limit {
                    crate::error::HttpBodyCaptureEnd::SizeLimit
                } else {
                    crate::error::HttpBodyCaptureEnd::Complete
                };
                let preview_truncated = body.len() > limit;
                let raw = body.slice(..body.len().min(raw_limit));
                let preview = Self::render_error_body_preview(
                    &body[..body.len().min(limit)],
                    Some(body.len()),
                    preview_truncated,
                    content_type.as_ref(),
                    &self.options.log_redactor,
                );
                return Ok(HttpErrorBodyCapture {
                    preview,
                    raw_body: raw,
                    end,
                    error: None,
                });
            }
            HttpResponseBodyState::StreamingTaken => {
                return Ok(HttpErrorBodyCapture {
                    preview: "<empty>".to_string(),
                    raw_body: Bytes::new(),
                    end: crate::error::HttpBodyCaptureEnd::BodyUnavailable,
                    error: None,
                });
            }
        };
        self.read_error_body_preview(backend, limit, raw_limit, content_type)
            .await
    }

    /// Reads bounded preview and raw capture bytes from a response body for
    /// status errors, tracking their truncation independently. When the
    /// original declared length exceeds both capture limits, capture may stop
    /// once both retained prefixes reach their limits; this does not verify
    /// complete transport of the declared body. The read timeout is a
    /// cumulative budget starting when this capture begins; if it expires, the
    /// caller retains the status and any partial raw body marked incomplete.
    ///
    /// # Errors
    /// Returns [`HttpErrorKind::Cancelled`](crate::HttpErrorKind::Cancelled)
    /// when the supplied cancellation token fires while waiting for preview
    /// bytes.
    ///
    /// # Parameters
    /// - `response`: Backend response whose error body is read.
    /// - `max_bytes`: Maximum rendered preview size in bytes.
    /// - `raw_limit`: Maximum retained raw body size in bytes.
    /// - `content_type`: Optional response content type used to render preview.
    ///
    /// # Returns
    /// Captured response body data and its terminal status.
    async fn read_error_body_preview(
        &self,
        mut response: reqwest::Response,
        max_bytes: usize,
        raw_limit: usize,
        content_type: Option<HeaderValue>,
    ) -> HttpResult<HttpErrorBodyCapture> {
        let limit = max_bytes.max(1);
        let read_timeout = self.runtime.read_timeout;
        let cancellation_token = self.runtime.cancellation_token.clone();
        let method = self.meta.method().clone();
        let url = self.runtime.request_url.clone();
        let status = self.meta.status();
        let source_len = self
            .content_length_hint()
            .and_then(|length| usize::try_from(length).ok());
        let mut preview = Vec::new();
        let mut raw = Vec::new();
        let mut preview_truncated = false;
        let mut raw_truncated = false;
        let mut end = crate::error::HttpBodyCaptureEnd::Complete;
        let capture_limit = limit.max(raw_limit.max(1)).saturating_add(1);
        let mut capture_budget =
            ResourceBudget::new("status error response body preview", capture_limit);
        let capture_started_at = Instant::now();

        loop {
            if cancellation_token
                .as_ref()
                .is_some_and(HttpCancellationToken::is_cancelled)
            {
                return Err(HttpError::cancelled(
                    "Request cancelled while reading status error response body preview",
                )
                .with_method(&method)
                .with_url(&url)
                .with_status(status)
                .with_log_redactor(self.log_redactor().clone()));
            }
            let remaining = read_timeout.saturating_sub(capture_started_at.elapsed());
            if remaining.is_zero() {
                return Ok(HttpErrorBodyCapture {
                    preview: format!(
                        "<error body unavailable: read timeout after {:?}>",
                        read_timeout
                    ),
                    raw_body: Bytes::from(raw),
                    end: crate::error::HttpBodyCaptureEnd::ReadTimeout,
                    error: None,
                });
            }
            let next = if let Some(token) = cancellation_token.as_ref() {
                tokio::select! {
                    _ = token.cancelled() => {
                        return Err(HttpError::cancelled(
                            "Request cancelled while reading status error response body preview",
                        )
                        .with_method(&method)
                        .with_url(&url)
                        .with_status(status)
                        .with_log_redactor(
                            self.log_redactor().clone(),
                        ));
                    }
                    item = tokio::time::timeout(remaining, response.chunk()) => item,
                }
            } else {
                tokio::time::timeout(remaining, response.chunk()).await
            };
            match next {
                Ok(Ok(Some(chunk))) => {
                    let captured = capture_budget.consume_available(chunk.len());
                    let preview_remaining = limit.saturating_sub(preview.len());
                    if preview_remaining < captured {
                        preview_truncated = true;
                    }
                    preview.extend_from_slice(&chunk[..captured.min(preview_remaining)]);
                    let raw_remaining = raw_limit.saturating_sub(raw.len());
                    if raw_remaining < chunk.len() {
                        raw_truncated = true;
                    }
                    raw.extend_from_slice(&chunk[..chunk.len().min(raw_remaining)]);
                    if captured < chunk.len() {
                        preview_truncated |= preview_remaining < chunk.len();
                        raw_truncated |= raw_remaining < chunk.len();
                        break;
                    }
                    let capture_targets_satisfied =
                        preview.len() == limit && raw.len() == raw_limit;
                    let declared_truncation =
                        source_len.is_some_and(|length| length > limit.max(raw_limit));
                    if capture_targets_satisfied && declared_truncation {
                        break;
                    }
                    if capture_budget.used() == capture_limit
                        && (preview_truncated || raw_truncated)
                    {
                        break;
                    }
                }
                Ok(Ok(None)) => break,
                Ok(Err(error)) => {
                    let error = error.without_url();
                    let preview =
                        "<error body unavailable: failed to read response body>".to_string();
                    let capture_error: Arc<dyn Error + Send + Sync + 'static> = Arc::new(error);
                    end = crate::error::HttpBodyCaptureEnd::ReadFailure;
                    return Ok(HttpErrorBodyCapture {
                        preview,
                        raw_body: Bytes::from(raw),
                        end,
                        error: Some(capture_error),
                    });
                }
                Err(_) => {
                    return Ok(HttpErrorBodyCapture {
                        preview: format!(
                            "<error body unavailable: read timeout after {:?}>",
                            read_timeout
                        ),
                        raw_body: Bytes::from(raw),
                        end: crate::error::HttpBodyCaptureEnd::ReadTimeout,
                        error: None,
                    });
                }
            }
        }
        if preview.len() > limit {
            preview.truncate(limit);
            preview_truncated = true;
        }
        preview_truncated |= source_len.is_some_and(|length| length > limit);
        raw_truncated |= source_len.is_some_and(|length| length > raw_limit);
        if raw_truncated {
            end = crate::error::HttpBodyCaptureEnd::SizeLimit;
        }
        Ok(HttpErrorBodyCapture {
            preview: Self::render_error_body_preview(
                &preview,
                source_len,
                preview_truncated,
                content_type.as_ref(),
                &self.options.log_redactor,
            ),
            raw_body: Bytes::from(raw),
            end,
            error: None,
        })
    }

    /// Renders captured status-error bytes through the response redactor.
    ///
    /// # Parameters
    ///
    /// * `bytes` - Captured body prefix.
    /// * `source_len` - Exact complete source length when known.
    /// * `truncated` - Whether source bytes were omitted from the capture.
    /// * `content_type` - Optional native Content-Type used for parser choice.
    /// * `log_redactor` - Response-scoped redactor and policy snapshot.
    ///
    /// # Returns
    ///
    /// `<empty>` for a complete empty body, otherwise bounded log-safe text.
    fn render_error_body_preview(
        bytes: &[u8],
        source_len: Option<usize>,
        truncated: bool,
        content_type: Option<&HeaderValue>,
        log_redactor: &Redactor,
    ) -> String {
        if bytes.is_empty() && !truncated {
            return "<empty>".to_owned();
        }
        let capture = if truncated {
            source_len.map_or_else(
                || BodyCapture::truncated_unknown(bytes),
                |total_len| {
                    BodyCapture::truncated(bytes, total_len)
                        .unwrap_or_else(|_| BodyCapture::truncated_unknown(bytes))
                },
            )
        } else {
            BodyCapture::complete(bytes)
        };
        let body = log_redactor.redact_http_body(capture, content_type);
        body.into_text_or_marker("<redaction incomplete>")
            .into_string()
    }

    /// Extracts a Content-Type header value.
    ///
    /// # Parameters
    /// - `headers`: Headers to inspect.
    ///
    /// # Returns
    /// Owned Content-Type value when present, including non-UTF-8 values.
    fn content_type_header(headers: &HeaderMap) -> Option<HeaderValue> {
        headers.get(CONTENT_TYPE).cloned()
    }
}
