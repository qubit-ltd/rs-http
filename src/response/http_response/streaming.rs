// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! One-shot backend response streaming and cached-body streams.
use async_stream::stream;
use futures_util::stream as futures_stream;
use futures_util::StreamExt;

use super::super::internal::http_response_runtime::remember_body_read_failure_state;
use super::super::internal::HttpResponseBodyState;
use super::map_response_read_error;
use super::HttpResponse;
use crate::HttpByteStream;
use crate::HttpError;
use crate::HttpResult;

impl HttpResponse {
    /// Returns body as stream; if already buffered, returns a stream backed by
    /// cached bytes without consuming the cache. Repeated streams and byte
    /// reads therefore return the same buffered body.
    ///
    /// The stream takes ownership of an unbuffered backend body. It can only be
    /// obtained once; a buffered body can be streamed from its cached bytes.
    ///
    /// # Returns
    /// A stream of body chunks. A successfully buffered body remains readable
    /// after request cancellation; cancellation stops reads that still require
    /// backend I/O.
    ///
    /// # Errors
    /// Returns an error if the body was already consumed, an earlier read
    /// failed, or an unbuffered response was cancelled before streaming starts.
    pub fn stream(&mut self) -> HttpResult<HttpByteStream> {
        if let Some(error) = self.previous_body_read_error() {
            return Err(error);
        }
        if let HttpResponseBodyState::Buffered(body) = &self.body {
            let body = body.clone();
            return Ok(Box::pin(futures_stream::once(async move { Ok(body) })));
        }
        if let Some(error) = self
            .cancelled_error_if_needed("Streaming response cancelled before reading response body")
        {
            return Err(error);
        }
        let state = std::mem::replace(&mut self.body, HttpResponseBodyState::StreamingTaken);
        let backend = match state {
            HttpResponseBodyState::Buffered(_) => unreachable!("buffered body returned above"),
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
        let body_read_failure = self.runtime.body_read_failure.clone();
        let log_redactor = self.log_redactor().clone();
        let mut stream = backend.bytes_stream();
        let wrapped = stream! {
            loop {
                let next = if let Some(token) = &cancellation_token {
                    tokio::select! {
                        _ = token.cancelled() => {
                            let error = HttpError::cancelled("Streaming response cancelled while reading body")
                                .with_method(&method)
                                .with_url(&url)
                                .with_status(status)
                                .with_log_redactor(log_redactor.clone());
                            remember_body_read_failure_state(&body_read_failure, &error);
                            yield Err(error);
                            break;
                        }
                        item = tokio::time::timeout(read_timeout, stream.next()) => item,
                    }
                } else {
                    tokio::time::timeout(read_timeout, stream.next()).await
                };
                match next {
                    Ok(Some(Ok(bytes))) => yield Ok(bytes),
                    Ok(Some(Err(error))) => {
                        let mapped = map_response_read_error(
                            error,
                            method.clone(),
                            url.clone(),
                            status,
                            &log_redactor,
                        );
                        remember_body_read_failure_state(&body_read_failure, &mapped);
                        yield Err(mapped);
                        break;
                    }
                    Ok(None) => break,
                    Err(_) => {
                        let error = HttpError::read_timeout(format!(
                            "Read timeout after {:?} while streaming response",
                            read_timeout
                        ))
                        .with_method(&method)
                        .with_url(&url)
                        .with_status(status)
                        .with_log_redactor(log_redactor.clone());
                        remember_body_read_failure_state(&body_read_failure, &error);
                        yield Err(error);
                        break;
                    }
                }
            }
        };
        Ok(Box::pin(wrapped))
    }
}
