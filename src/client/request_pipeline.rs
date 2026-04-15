/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Request preparation/sending and response-body handling pipeline helpers.

use reqwest::Response;

use crate::client::error_mapper::{map_reqwest_error, parse_retry_after};
use crate::{HttpClient, HttpErrorKind, HttpLogger, HttpRequest, HttpResponse, HttpResult};

/// Pipeline object that encapsulates one-attempt request setup and response
/// handling.
pub(super) struct RequestPipeline<'a> {
    client: &'a HttpClient,
}

impl<'a> RequestPipeline<'a> {
    /// Creates a new pipeline bound to one [`HttpClient`] instance.
    ///
    /// # Parameters
    /// - `client`: Parent HTTP client that provides options and low-level
    ///   sender.
    ///
    /// # Returns
    /// New request pipeline wrapper.
    pub(super) fn new(client: &'a HttpClient) -> Self {
        Self { client }
    }

    /// Resolves URL, applies headers/query/body/timeout, logs request, then
    /// sends one attempt.
    ///
    /// # Parameters
    /// - `request`: Request to execute.
    /// - `cancellation_message`: Error message used when cancelled before send.
    ///
    /// # Returns
    /// `(request, response)` where `request` is the request snapshot used in this
    /// attempt and `response` is the raw reqwest response.
    pub(super) async fn prepare_and_send_once(
        &self,
        request: HttpRequest,
        cancellation_message: &str,
    ) -> HttpResult<(HttpRequest, Response)> {
        let mut request = request;
        if let Some(error) = request.cancelled_error_if_needed(cancellation_message) {
            return Err(error);
        }
        request.invalidate_effective_headers_cache();
        request.effective_headers().await?;

        let logger = HttpLogger::new(&self.client.options);
        logger.log_request(&request);

        let response = request.send_impl(&self.client.backend).await?;
        Ok((request, response))
    }

    /// Converts a non-success response into [`HttpError`] with
    /// status/retry/body-preview context.
    ///
    /// # Parameters
    /// - `request`: Request snapshot used for this attempt (contains method and resolved URL).
    /// - `response`: Raw response from reqwest.
    /// - `message_prefix`: Prefix for the final error message.
    ///
    /// # Returns
    /// Original response when successful, otherwise mapped [`HttpError`].
    pub(super) async fn ensure_success_response(
        &self,
        request: &HttpRequest,
        response: Response,
        message_prefix: &str,
    ) -> HttpResult<Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let retry_after = parse_retry_after(status, response.headers());
        let error = response.error_for_status_ref().expect_err(
            "non-success HTTP status must produce reqwest status error via error_for_status_ref",
        );
        let body_preview = self.read_error_response_preview(response).await;

        let method = request.method();
        let url = request.resolved_url()?;

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

    /// Reads and renders a bounded preview for a non-success response body.
    ///
    /// # Parameters
    /// - `response`: Non-success response whose body will be consumed.
    ///
    /// # Returns
    /// Rendered preview text. On preview read failure, returns a descriptive
    /// placeholder.
    async fn read_error_response_preview(&self, response: Response) -> String {
        let read_timeout = self.client.options.timeouts.read_timeout;
        let max_bytes = self.client.options.error_response_preview_limit.max(1);
        HttpResponse::read_error_body_preview(response, read_timeout, max_bytes).await
    }
}
