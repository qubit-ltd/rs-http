/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Request preparation/sending and response-body handling pipeline helpers.

use bytes::Bytes;
use reqwest::Response;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::client::error_mapper::{
    map_reqwest_error, parse_retry_after, render_error_body_preview, ReqwestErrorPhase,
};
use crate::{HttpClient, HttpError, HttpErrorKind, HttpLogger, HttpRequest, HttpResult};

/// Shared pre-send outcome for one HTTP attempt.
pub(super) struct PreparedRequestSend {
    /// Request method used for this attempt.
    pub(super) method: http::Method,
    /// Resolved request URL used for this attempt.
    pub(super) url: Url,
    /// Optional cancellation token bound to this request.
    pub(super) cancellation_token: Option<CancellationToken>,
    /// Raw response returned by reqwest for this attempt.
    pub(super) response: Response,
}

/// Pipeline object that encapsulates one-attempt request setup and response handling.
pub(super) struct RequestPipeline<'a> {
    client: &'a HttpClient,
}

impl<'a> RequestPipeline<'a> {
    /// Creates a new pipeline bound to one [`HttpClient`] instance.
    ///
    /// # Parameters
    /// - `client`: Parent HTTP client that provides options and low-level sender.
    ///
    /// # Returns
    /// New request pipeline wrapper.
    pub(super) fn new(client: &'a HttpClient) -> Self {
        Self { client }
    }

    /// Resolves URL, applies headers/query/body/timeout, logs request, then sends one attempt.
    ///
    /// # Parameters
    /// - `request`: Request to execute.
    /// - `cancellation_message`: Error message used when cancelled before send.
    ///
    /// # Returns
    /// Request context and raw response for this attempt.
    pub(super) async fn prepare_and_send_once(
        &self,
        request: HttpRequest,
        cancellation_message: &str,
    ) -> HttpResult<PreparedRequestSend> {
        let mut request = request;
        let url = request.resolve_url()?;
        let method = request.method().clone();
        if let Some(error) = request.cancelled_error_if_needed(&url, cancellation_message) {
            return Err(error);
        }
        let headers = request.build_headers().await?;

        let logger = HttpLogger::new(&self.client.options);
        logger.log_request(&method, &url, &headers, request.body());

        let response = request
            .send_impl(&self.client.backend, &method, &url, headers)
            .await?;
        let cancellation_token = request.cancellation_token().cloned();
        Ok(PreparedRequestSend {
            method,
            url,
            cancellation_token,
            response,
        })
    }

    /// Converts a non-success response into [`HttpError`] with status/retry/body-preview context.
    ///
    /// # Parameters
    /// - `response`: Raw response from reqwest.
    /// - `method`: Request method used for this attempt.
    /// - `url`: Resolved request URL used for this attempt.
    /// - `message_prefix`: Prefix for the final error message.
    ///
    /// # Returns
    /// Original response when successful, otherwise mapped [`HttpError`].
    pub(super) async fn ensure_success_response(
        &self,
        response: Response,
        method: &http::Method,
        url: &Url,
        message_prefix: &str,
    ) -> HttpResult<Response> {
        if response.status().is_success() {
            return Ok(response);
        }

        let status = response.status();
        let retry_after = parse_retry_after(status, response.headers());
        let error = response.error_for_status_ref().expect_err(
            "non-success HTTP status must produce reqwest status error via error_for_status_ref",
        );
        let body_preview = self.read_error_response_preview(response).await;
        let message = format!(
            "{} with status {} for {} {}; response body preview: {}",
            message_prefix, status, method, url, body_preview
        );
        let mut mapped = map_reqwest_error(
            error,
            HttpErrorKind::Status,
            None,
            Some(method.clone()),
            Some(url.clone()),
        )
        .with_status(status)
        .with_response_body_preview(body_preview);
        if let Some(retry_after) = retry_after {
            mapped = mapped.with_retry_after(retry_after);
        }
        mapped.message = message;
        Err(mapped)
    }

    /// Reads the entire response body with a read timeout.
    ///
    /// # Parameters
    /// - `response`: Successful response whose body will be consumed.
    /// - `method`: Method for error context.
    /// - `url`: URL for error context.
    ///
    /// # Returns
    /// Body as [`Bytes`] or [`HttpError`] (decode/read timeout).
    pub(super) async fn read_body_with_timeout(
        &self,
        response: Response,
        method: http::Method,
        url: Url,
        cancellation_token: Option<&CancellationToken>,
    ) -> HttpResult<Bytes> {
        let timeout = self.client.options.timeouts.read_timeout;
        let read_future = tokio::time::timeout(timeout, response.bytes());
        let next = if let Some(token) = cancellation_token {
            tokio::select! {
                _ = token.cancelled() => {
                    return Err(HttpError::cancelled("Request cancelled while reading response body")
                        .with_method(method)
                        .with_url(url));
                }
                read_result = read_future => read_result,
            }
        } else {
            read_future.await
        };
        match next {
            Ok(Ok(body)) => Ok(body),
            Ok(Err(error)) => Err(map_reqwest_error(
                error,
                HttpErrorKind::Decode,
                Some(ReqwestErrorPhase::Read),
                Some(method),
                Some(url),
            )),
            Err(_) => Err(HttpError::read_timeout(format!(
                "Read timeout after {:?} while reading response body",
                timeout
            ))
            .with_method(method)
            .with_url(url)),
        }
    }

    /// Reads and renders a bounded preview for a non-success response body.
    ///
    /// # Parameters
    /// - `response`: Non-success response whose body will be consumed.
    ///
    /// # Returns
    /// Rendered preview text. On preview read failure, returns a descriptive placeholder.
    async fn read_error_response_preview(&self, mut response: Response) -> String {
        let read_timeout = self.client.options.timeouts.read_timeout;
        let max_bytes = self.client.options.error_response_preview_limit.max(1);
        let mut preview = Vec::new();
        let mut truncated = false;

        loop {
            let next = tokio::time::timeout(read_timeout, response.chunk()).await;
            match next {
                Ok(Ok(Some(chunk))) => {
                    if preview.len() >= max_bytes {
                        truncated = true;
                        break;
                    }
                    let remaining = max_bytes - preview.len();
                    if chunk.len() > remaining {
                        preview.extend_from_slice(&chunk[..remaining]);
                        truncated = true;
                        break;
                    }
                    preview.extend_from_slice(&chunk);
                }
                Ok(Ok(None)) => break,
                Ok(Err(error)) => {
                    return format!(
                        "<error body unavailable: failed to read response body: {}>",
                        error
                    );
                }
                Err(_) => {
                    return format!(
                        "<error body unavailable: read timeout after {:?}>",
                        read_timeout
                    );
                }
            }
        }

        render_error_body_preview(&preview, truncated)
    }

}
