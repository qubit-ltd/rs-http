// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded aggregate response body reads and decoding.
use bytes::Bytes;
use bytes::BytesMut;
use qubit_budget::ResourceBudget;
use qubit_json::decode::JsonDecoder;
use serde::de::DeserializeOwned;

use super::super::internal::HttpResponseBodyState;
use super::HttpResponse;
use super::map_response_read_error;
use crate::HttpError;
use crate::HttpResult;
use crate::json_limits::json_decode_limits;

impl HttpResponse {
    /// Returns full body bytes, consuming backend stream lazily on first call.
    ///
    /// # Returns
    /// The complete body bytes, or an error if reading fails or exceeds the
    /// configured limit. A successfully buffered body remains available to
    /// later calls.
    ///
    /// # Cancellation safety
    ///
    /// For an unbuffered response, dropping this future after it has taken the
    /// backend body and before buffering completes discards the backend and any
    /// partially read bytes. Later body readers return
    /// [`HttpErrorKind::ResponseBodyAlreadyConsumed`](crate::HttpErrorKind::ResponseBodyAlreadyConsumed). An outer
    /// `tokio::time::timeout` or a losing `tokio::select!` branch can cause
    /// this drop.
    ///
    /// Dropping a future before it is polled does not take the body. A
    /// successfully buffered body remains reusable. Cancellation and read
    /// timeouts returned by the built-in reader follow the existing
    /// retained-failure behavior instead. Configure the request's read timeout
    /// or cancellation token before sending when the reader should return a
    /// classified failure.
    /// Cancellation after buffering does not prevent reads of cached bytes.
    ///
    /// # Errors
    /// Returns the backend read, timeout, cancellation, or body-size error.
    pub async fn bytes(&mut self) -> HttpResult<Bytes> {
        let body_limit = self.options.response_body_size_limit;
        if let Some(error) = self.previous_body_read_error() {
            return Err(error);
        }
        let state = std::mem::replace(&mut self.body, HttpResponseBodyState::StreamingTaken);
        let mut backend = match state {
            HttpResponseBodyState::Buffered(body) => {
                if body.len() > body_limit {
                    let error = self.response_body_size_limit_error(body.len());
                    self.remember_body_read_failure(&error);
                    return Err(error);
                }
                self.body = HttpResponseBodyState::Buffered(body.clone());
                return Ok(body);
            }
            HttpResponseBodyState::Backend(backend) => backend,
            HttpResponseBodyState::StreamingTaken => {
                return Err(self.response_body_already_consumed_error());
            }
        };

        let method = self.meta.method().clone();
        let url = self.runtime.request_url.clone();
        let status = self.meta.status();
        let read_timeout = self.runtime.read_timeout;
        let cancellation_token = self.runtime.cancellation_token.clone();
        if let Some(content_length) = self.content_length_hint() {
            let exceeds_limit = match usize::try_from(content_length) {
                Ok(observed_size) => observed_size > body_limit,
                Err(_) => true,
            };
            if exceeds_limit {
                let observed_size = usize::try_from(content_length).unwrap_or(usize::MAX);
                let error = self.response_body_size_limit_error(observed_size);
                self.remember_body_read_failure(&error);
                return Err(error);
            }
        }
        let mut body = BytesMut::new();
        let mut body_budget = ResourceBudget::new("response body", body_limit);

        loop {
            let next = if let Some(token) = &cancellation_token {
                tokio::select! {
                    _ = token.cancelled() => {
                        let error = HttpError::cancelled("Request cancelled while reading response body")
                            .with_method(&method)
                            .with_url(&url)
                            .with_status(status)
                            .with_log_redactor(
                                self.log_redactor().clone(),
                            );
                        self.remember_body_read_failure(&error);
                        return Err(error);
                    }
                    item = tokio::time::timeout(read_timeout, backend.chunk()) => item,
                }
            } else {
                tokio::time::timeout(read_timeout, backend.chunk()).await
            };

            match next {
                Ok(Ok(Some(chunk))) => {
                    if let Err(error) = body_budget.try_consume(chunk.len()) {
                        let observed_size = error.used().saturating_add(error.requested());
                        let error = self.response_body_size_limit_error(observed_size);
                        self.remember_body_read_failure(&error);
                        return Err(error);
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(Ok(None)) => {
                    let body = body.freeze();
                    self.body = HttpResponseBodyState::Buffered(body.clone());
                    return Ok(body);
                }
                Ok(Err(error)) => {
                    let error = map_response_read_error(error, method, url, status, self.log_redactor());
                    self.remember_body_read_failure(&error);
                    return Err(error);
                }
                Err(_) => {
                    let error = HttpError::read_timeout(format!(
                        "Read timeout after {:?} while reading response body",
                        read_timeout
                    ))
                    .with_method(self.meta.method())
                    .with_url(&self.runtime.request_url)
                    .with_status(status)
                    .with_log_redactor(self.log_redactor().clone());
                    self.remember_body_read_failure(&error);
                    return Err(error);
                }
            }
        }
    }

    /// Interprets the complete response body as UTF-8 text.
    ///
    /// # Returns
    /// Decoded text when the body contains valid UTF-8.
    ///
    /// # Errors
    /// Returns a body read error or a decoding error for invalid UTF-8.
    ///
    /// # Cancellation safety
    ///
    /// This method aggregates through [`Self::bytes`] and has the same
    /// cancellation-safety boundary: dropping a polled read future before
    /// buffering completes can consume an unbuffered backend body permanently.
    pub async fn text(&mut self) -> HttpResult<String> {
        let body = self.bytes().await?;
        String::from_utf8(body.to_vec()).map_err(|error| {
            HttpError::decode(format!("Failed to decode response body as UTF-8: {}", error))
                .with_status(self.meta.status())
                .with_method(self.meta.method())
                .with_url(self.request_url())
                .with_log_redactor(self.log_redactor().clone())
        })
    }

    /// Deserializes the complete response body as JSON into `T`.
    ///
    /// # Type Parameters
    /// - `T`: Owned type to deserialize from the response JSON.
    ///
    /// # Returns
    /// The decoded value when the body is valid JSON matching `T`.
    ///
    /// # Errors
    /// Returns a body read error or a JSON decoding error.
    ///
    /// # Cancellation safety
    ///
    /// This method aggregates through [`Self::bytes`] and has the same
    /// cancellation-safety boundary: dropping a polled read future before
    /// buffering completes can consume an unbuffered backend body permanently.
    pub async fn json<T>(&mut self) -> HttpResult<T>
    where
        T: DeserializeOwned,
    {
        let body = self.bytes().await?;
        JsonDecoder::with_limits(json_decode_limits(
            self.options.response_body_size_limit,
            self.options.json_value_limits,
        ))
        .decode_utf8(&body)
        .map_err(|error| {
            HttpError::decode("Failed to decode response JSON")
                .with_status(self.meta.status())
                .with_method(self.meta.method())
                .with_url(self.request_url())
                .with_source(error)
                .with_log_redactor(self.log_redactor().clone())
        })
    }
}
