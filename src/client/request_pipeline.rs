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
use futures_util::stream as futures_stream;
use http::HeaderMap;
use qubit_function::MutatingFunction;
use reqwest::Response;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::client::error_mapper::{
    map_reqwest_error, parse_retry_after, render_error_body_preview, ReqwestErrorPhase,
};
use crate::{
    HttpClient, HttpError, HttpErrorKind, HttpLogger, HttpRequest, HttpRequestBody, HttpResult,
};

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
        let url = self.client.resolve_url(&request)?;
        let method = request.method.clone();
        let cancellation_token = request.cancellation_token.clone();
        if let Some(error) = cancelled_request_error_if_needed(
            cancellation_token.as_ref(),
            &method,
            &url,
            cancellation_message,
        ) {
            return Err(error);
        }
        let headers = self.build_headers(&request).await?;
        let body_for_log = clone_request_body_for_log(&request.body);

        let logger = HttpLogger::new(&self.client.options);
        logger.log_request(&method, &url, &headers, body_for_log.as_ref());

        let mut builder = self.client.client.request(method.clone(), url.clone());
        builder = builder.headers(headers);
        if !request.query.is_empty() {
            builder = builder.query(&request.query);
        }
        if let Some(timeout) = request.request_timeout {
            builder = builder.timeout(timeout);
        }
        builder = apply_request_body(builder, request.body);

        let response = self
            .send_with_write_timeout(
                builder,
                method.clone(),
                url.clone(),
                cancellation_token.as_ref(),
            )
            .await?;
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

    /// Merges default headers, injector output, and per-request headers (later
    /// wins on duplicates).
    ///
    /// # Parameters
    /// - `request`: Request supplying extra headers.
    ///
    /// # Returns
    /// Final [`HeaderMap`] or error if an injector fails.
    async fn build_headers(&self, request: &HttpRequest) -> HttpResult<HeaderMap> {
        let mut headers = self.client.options.default_headers.clone();

        for injector in &self.client.injectors {
            injector.apply(&mut headers)?;
        }
        for injector in &self.client.async_injectors {
            injector.apply(&mut headers).await?;
        }

        headers.extend(request.headers.clone());
        Ok(headers)
    }

    /// Sends the built request with a write-phase timeout (time to finish
    /// sending the request).
    ///
    /// # Parameters
    /// - `builder`: Reqwest request builder (method, URL, headers, body already
    ///   set).
    /// - `method`: Method for error context.
    /// - `url`: URL for error context.
    ///
    /// # Returns
    /// Raw [`reqwest::Response`] or [`HttpError`] (transport, write timeout,
    /// etc.).
    async fn send_with_write_timeout(
        &self,
        builder: reqwest::RequestBuilder,
        method: http::Method,
        url: Url,
        cancellation_token: Option<&CancellationToken>,
    ) -> HttpResult<Response> {
        let timeout = self.client.options.timeouts.write_timeout;
        let send_future = tokio::time::timeout(timeout, builder.send());
        let next = if let Some(token) = cancellation_token {
            tokio::select! {
                _ = token.cancelled() => {
                    return Err(HttpError::cancelled("Request cancelled while sending")
                        .with_method(method)
                        .with_url(url));
                }
                send_result = send_future => send_result,
            }
        } else {
            send_future.await
        };
        match next {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(error)) => Err(map_reqwest_error(
                error,
                HttpErrorKind::Transport,
                Some(ReqwestErrorPhase::Send),
                Some(method),
                Some(url),
            )),
            Err(_) => Err(HttpError::write_timeout(format!(
                "Write timeout after {:?} while sending request",
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

/// Builds a cancelled error when `token` is already cancelled.
///
/// # Parameters
/// - `token`: Optional cancellation token for this request.
/// - `method`: Request method for error context.
/// - `url`: Request URL for error context.
/// - `message`: Cancellation message.
///
/// # Returns
/// `Some(HttpError)` when cancelled, otherwise `None`.
fn cancelled_request_error_if_needed(
    token: Option<&CancellationToken>,
    method: &http::Method,
    url: &Url,
    message: &str,
) -> Option<HttpError> {
    if token.is_some_and(CancellationToken::is_cancelled) {
        Some(
            HttpError::cancelled(message.to_string())
                .with_method(method.clone())
                .with_url(url.clone()),
        )
    } else {
        None
    }
}

/// Clones request body content for request logging.
///
/// # Parameters
/// - `body`: Request body variant.
///
/// # Returns
/// Optional byte payload for logger previewing.
fn clone_request_body_for_log(body: &HttpRequestBody) -> Option<Bytes> {
    match body {
        HttpRequestBody::Bytes(bytes)
        | HttpRequestBody::Json(bytes)
        | HttpRequestBody::Form(bytes)
        | HttpRequestBody::Multipart(bytes)
        | HttpRequestBody::Ndjson(bytes) => Some(bytes.clone()),
        HttpRequestBody::Text(text) => Some(Bytes::from(text.clone())),
        HttpRequestBody::Stream(_) => None,
        HttpRequestBody::Empty => None,
    }
}

/// Applies request body variant to a reqwest request builder.
///
/// # Parameters
/// - `builder`: Request builder with method/url/headers/query already set.
/// - `body`: Request body variant to apply.
///
/// # Returns
/// Updated builder containing the request body payload.
fn apply_request_body(
    builder: reqwest::RequestBuilder,
    body: HttpRequestBody,
) -> reqwest::RequestBuilder {
    match body {
        HttpRequestBody::Empty => builder,
        HttpRequestBody::Bytes(bytes)
        | HttpRequestBody::Json(bytes)
        | HttpRequestBody::Form(bytes)
        | HttpRequestBody::Multipart(bytes)
        | HttpRequestBody::Ndjson(bytes) => builder.body(bytes),
        HttpRequestBody::Stream(chunks) => {
            let body_stream =
                futures_stream::iter(chunks.into_iter().map(Result::<Bytes, std::io::Error>::Ok));
            builder.body(reqwest::Body::wrap_stream(body_stream))
        }
        HttpRequestBody::Text(text) => builder.body(text),
    }
}
