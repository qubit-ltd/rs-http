// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # HTTP Logger
//!
//! Encapsulates request and response logging behavior.

use http::HeaderValue;
use http::header::CONTENT_TYPE;
use qubit_redact::RedactionCompletion;
use qubit_redact::RedactionSession;
use qubit_redact::formats::http::BodyRedaction;
use qubit_redact::formats::http::HttpRedactor;
use tracing::Metadata;
use tracing::callsite::DefaultCallsite;
use tracing::metadata::Kind;

use crate::HttpClientOptions;
use crate::HttpLoggingOptions;
use crate::HttpRequest;
use crate::HttpRequestBody;
use crate::HttpResponse;
use crate::HttpResponseMeta;
use crate::redact::RedactedLogger;

const UNRESOLVED_REQUEST_URL: &str = "<unresolved request URL>";
const STREAMING_REQUEST_BODY_SKIPPED: &str =
    "<skipped: streaming request body>";
const REDACTION_EXHAUSTED: &str = "<truncated>";

/// Callsite metadata used to query the current dispatcher directly, bypassing
/// shared interest caching that can change while thread-local subscribers are
/// installed concurrently.
static HTTP_LOGGER_ENABLED_CALLSITE: DefaultCallsite =
    DefaultCallsite::new(&HTTP_LOGGER_ENABLED_METADATA);
static HTTP_LOGGER_ENABLED_METADATA: Metadata<'static> = tracing::metadata! {
    name: "qubit_http_logger_enabled",
    target: module_path!(),
    level: tracing::Level::TRACE,
    fields: &[],
    callsite: &HTTP_LOGGER_ENABLED_CALLSITE,
    kind: Kind::EVENT,
};

/// HTTP logger bound to one pair of logging options and a redactor policy.
#[derive(Debug, Clone)]
pub struct HttpLogger<'a> {
    options: &'a HttpLoggingOptions,
    redacted_logger: RedactedLogger,
}

/// Request body preview category used by TRACE logging.
enum RequestBodyLogPreview<'a> {
    /// Borrowed bytes that can be safely previewed without consuming a stream.
    Bytes(&'a [u8]),
    /// No request body is present.
    Empty,
    /// Body logging is intentionally skipped.
    Skipped(&'static str),
}

impl<'a> HttpLogger<'a> {
    /// Creates a logger view from one client option object.
    ///
    /// # Parameters
    /// - `options`: Client options that carry logging switches and sensitive
    ///   header policies.
    ///
    /// # Returns
    /// A logger that emits TRACE records according to the provided options.
    pub fn new(options: &'a HttpClientOptions) -> Self {
        let log_redactor =
            HttpRedactor::new(options.log_redaction_policy.clone());
        Self::from_options_with_redactor(options, log_redactor)
    }

    /// Creates a logger bound to the caller's shared redactor snapshot.
    ///
    /// # Parameters
    ///
    /// * `options` - Client options carrying logging switches.
    /// * `log_redactor` - Exact redactor propagated by the owning client.
    ///
    /// # Returns
    ///
    /// A logger that shares the caller's immutable redactor snapshot.
    pub(crate) fn from_options_with_redactor(
        options: &'a HttpClientOptions,
        log_redactor: HttpRedactor,
    ) -> Self {
        Self {
            options: &options.logging,
            redacted_logger: RedactedLogger::from_options_with_redactor(
                options,
                log_redactor,
            ),
        }
    }

    /// Emits TRACE logs for an outbound request when logging is enabled and
    /// TRACE is active.
    ///
    /// # Parameters
    /// - `request`: Prepared request snapshot; expected to carry resolved URL
    ///   and attempt-level merged headers.
    ///
    /// # Returns
    /// Nothing; no-op when disabled or TRACE off.
    pub fn log_request(&self, request: &HttpRequest) {
        if !self.is_trace_enabled() {
            return;
        }

        let mut session = self.redacted_logger.session();
        let url = self.request_log_url(request, &mut session);
        tracing::trace!("--> {} {}", request.method(), url);

        let headers = request
            .effective_headers_cached()
            .unwrap_or_else(|| request.headers());

        if self.options.log_request_header {
            tracing::trace!("{}", session.http().redact_headers(headers));
        }

        if self.options.log_request_body {
            match Self::request_body_for_log(request) {
                RequestBodyLogPreview::Bytes(bytes) => {
                    let content_type = Self::content_type(headers);
                    tracing::trace!(
                        "Request body: {}",
                        self.body_log_text(bytes, content_type, &mut session)
                    );
                }
                RequestBodyLogPreview::Empty => {
                    tracing::trace!("Request body: <empty>")
                }
                RequestBodyLogPreview::Skipped(reason) => {
                    tracing::trace!("Request body: {reason}")
                }
            }
        }
    }

    /// Emits TRACE logs for a completed response (headers and optional body
    /// preview).
    ///
    /// # Parameters
    /// - `response`: Response object (status/url/headers/body cache).
    ///
    /// # Returns
    /// `Ok(())` on success; no-op when disabled or TRACE off.
    ///
    /// # Errors
    /// Returns [`crate::HttpError`] when reading the response body for logging
    /// fails.
    pub async fn log_response(
        &self,
        response: &mut HttpResponse,
    ) -> crate::HttpResult<()> {
        if !self.is_trace_enabled() {
            return Ok(());
        }

        let mut session = self.redacted_logger.session();
        tracing::trace!(
            "<-- {} {}",
            response.status().as_u16(),
            session.http().redact_url(response.url())
        );

        if self.options.log_response_header {
            tracing::trace!(
                "{}",
                session.http().redact_headers(response.headers())
            );
        }

        if self.options.log_response_body {
            let content_type = Self::content_type(response.headers()).cloned();
            if let Some(body) = response.buffered_body_for_logging() {
                tracing::trace!(
                    "Response body: {}",
                    self.body_log_text(
                        body.as_ref(),
                        content_type.as_ref(),
                        &mut session
                    )
                );
            } else if response
                .can_buffer_body_for_logging(self.options.body_size_limit)
            {
                let body = response.bytes().await?;
                tracing::trace!(
                    "Response body: {}",
                    self.body_log_text(
                        body.as_ref(),
                        content_type.as_ref(),
                        &mut session
                    )
                );
            } else {
                tracing::trace!(
                    "Response body: <skipped: streaming or unknown-size body>"
                );
            }
        }
        Ok(())
    }

    /// Logs response line and headers for a streaming call without reading the
    /// body stream.
    ///
    /// # Parameters
    /// - `response_meta`: Response metadata (status/url/headers).
    ///
    /// # Returns
    /// Nothing; no-op when disabled or TRACE off.
    pub fn log_stream_response_headers(
        &self,
        response_meta: &HttpResponseMeta,
    ) {
        if !self.is_trace_enabled() {
            return;
        }

        let mut session = self.redacted_logger.session();
        tracing::trace!(
            "<-- {} {} (stream)",
            response_meta.status().as_u16(),
            session.http().redact_url(response_meta.url())
        );

        if self.options.log_response_header {
            tracing::trace!(
                "{}",
                session.http().redact_headers(response_meta.headers())
            );
        }
    }

    /// Returns whether TRACE logs should be emitted under current options and
    /// subscriber state.
    ///
    /// # Returns
    /// `true` when logging is enabled and TRACE is active.
    pub fn is_trace_enabled(&self) -> bool {
        self.options.enabled
            && tracing::dispatcher::get_default(|dispatcher| {
                dispatcher.enabled(&HTTP_LOGGER_ENABLED_METADATA)
            })
    }

    /// Returns the URL text used by request logging.
    ///
    /// # Parameters
    /// - `request`: Request whose resolved URL should be rendered.
    ///
    /// # Returns
    /// Resolved URL including builder query parameters, or a fixed placeholder
    /// when URL resolution fails before send.
    fn request_log_url(
        &self,
        request: &HttpRequest,
        session: &mut RedactionSession<'_>,
    ) -> String {
        request
            .resolved_url()
            .map(|url| session.http().redact_url(&url))
            .map(|url| url.into_owned())
            .unwrap_or_else(|_| UNRESOLVED_REQUEST_URL.to_string())
    }

    /// Returns log text for a body while preserving structured completion
    /// until the presentation boundary.
    ///
    /// # Parameters
    ///
    /// * `body` - Complete body bytes offered by the logging layer.
    /// * `content_type` - Optional native Content-Type used for parser choice.
    /// * `session` - Shared redaction session for the enclosing TRACE record.
    ///
    /// # Returns
    ///
    /// `<empty>` for an empty source body. For a non-empty body, `Complete`
    /// preserves the complete log-safe text, `Truncated` preserves its
    /// non-empty safe substitute, and `Exhausted` maps empty adapter output to
    /// [`REDACTION_EXHAUSTED`]. Body status is independent of this completion
    /// mapping. An exhausted session has terminated input processing, so the
    /// body adapter does not read further source bytes.
    fn body_log_text(
        &self,
        body: &[u8],
        content_type: Option<&HeaderValue>,
        session: &mut RedactionSession<'_>,
    ) -> String {
        if body.is_empty() {
            return "<empty>".to_owned();
        }
        Self::render_body_redaction(self.redacted_logger.body(
            body,
            content_type,
            session,
        ))
    }

    /// Maps structured completion to the final logger presentation.
    ///
    /// Body status intentionally remains independent: it describes how the
    /// representation was produced, while only completion determines the
    /// presentation. `Complete` preserves complete safe text, `Truncated`
    /// preserves the non-empty safe substitute, and `Exhausted` maps its empty
    /// text to [`REDACTION_EXHAUSTED`]. Exhaustion is terminal for input
    /// processing, and the producing adapter has stopped without reading
    /// further source bytes.
    ///
    /// # Parameters
    ///
    /// * `redaction` - Structured body result from the shared session.
    ///
    /// # Returns
    ///
    /// Complete or truncated log-safe text as described above, or the outer
    /// marker for exhausted results.
    fn render_body_redaction(redaction: BodyRedaction) -> String {
        match redaction.completion() {
            RedactionCompletion::Complete | RedactionCompletion::Truncated => {
                redaction.into_log_safe_text().into_owned()
            }
            RedactionCompletion::Exhausted => REDACTION_EXHAUSTED.to_owned(),
        }
    }

    /// Borrows request body content only when body logging is safe.
    ///
    /// # Parameters
    /// - `request`: Prepared request snapshot.
    ///
    /// # Returns
    /// Body preview category for logger rendering.
    fn request_body_for_log(
        request: &HttpRequest,
    ) -> RequestBodyLogPreview<'_> {
        if request.has_streaming_body() {
            return RequestBodyLogPreview::Skipped(
                STREAMING_REQUEST_BODY_SKIPPED,
            );
        }
        match request.body() {
            HttpRequestBody::Bytes(bytes)
            | HttpRequestBody::Json(bytes)
            | HttpRequestBody::Form(bytes)
            | HttpRequestBody::Multipart(bytes)
            | HttpRequestBody::Ndjson(bytes) => {
                RequestBodyLogPreview::Bytes(bytes.as_ref())
            }
            HttpRequestBody::Text(text) => {
                RequestBodyLogPreview::Bytes(text.as_bytes())
            }
            HttpRequestBody::Stream(_) => {
                RequestBodyLogPreview::Skipped(STREAMING_REQUEST_BODY_SKIPPED)
            }
            HttpRequestBody::Empty => RequestBodyLogPreview::Empty,
        }
    }

    /// Extracts a Content-Type header value from a header map.
    ///
    /// # Parameters
    /// - `headers`: Headers to inspect.
    ///
    /// # Returns
    /// `Some` when Content-Type is present, including when its value is not
    /// valid UTF-8; otherwise `None`.
    fn content_type(headers: &http::HeaderMap) -> Option<&http::HeaderValue> {
        headers.get(CONTENT_TYPE)
    }
}
