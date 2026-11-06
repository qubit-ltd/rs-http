// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # SSE JSON Decoder
//!
//! Converts SSE events into typed JSON chunks.

use async_stream::stream;
use futures_util::StreamExt;
use futures_util::stream::once;
use qubit_budget::json::JsonValueLimits;
use serde::de::DeserializeOwned;

use super::DoneMarkerPolicy;
use super::SseChunk;
use super::SseChunkStream;
use super::SseCompletionPolicy;
use super::SseJsonMode;
use super::decode_messages_from_stream_with_limits;
use super::internal::decode_poll_budget::DecodePollBudget;
use crate::HttpByteStream;
use crate::HttpError;
use crate::json_limits::json_decode_limits;

/// Parses SSE JSON payloads with selectable strictness and explicit line/frame
/// size limits.
///
/// # Type Parameters
/// - `T`: JSON type to deserialize.
///
/// # Parameters
/// - `stream`: SSE HTTP body byte stream.
/// - `done_policy`: Terminal marker detection.
/// - `mode`: Lenient vs strict JSON parsing.
/// - `max_line_bytes`: Maximum allowed bytes for one SSE line.
/// - `max_frame_bytes`: Maximum allowed bytes for one SSE frame.
/// - `json_value_limits`: Structural and decoded-payload limits for each JSON
///   event.
/// - `completion_policy`: Whether ordinary end-of-stream is accepted without a
///   done marker.
///
/// # Returns
/// Stream of [`SseChunk::Data`] or [`SseChunk::Done`], or errors from
/// upstream/SSE/JSON.
#[must_use]
pub(crate) fn decode_json_chunks_from_stream_with_limits<T>(
    stream: HttpByteStream,
    done_policy: DoneMarkerPolicy,
    mode: SseJsonMode,
    max_line_bytes: usize,
    max_frame_bytes: usize,
    json_value_limits: JsonValueLimits,
    completion_policy: SseCompletionPolicy,
) -> SseChunkStream<T>
where
    T: DeserializeOwned + Send + 'static,
{
    if matches!(completion_policy, SseCompletionPolicy::RequireDoneMarker) && !done_policy.has_usable_done_marker() {
        return Box::pin(once(async {
            Err(HttpError::sse_protocol(
                "RequireDoneMarker needs a non-empty enabled done marker",
            ))
        }));
    }
    let mut messages = decode_messages_from_stream_with_limits(stream, max_line_bytes, max_frame_bytes);
    let output = stream! {
        let mut poll_budget = DecodePollBudget::default();
        while let Some(item) = messages.next().await {
            let message = match item {
                Ok(message) => message,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            };

            poll_budget.account(1, message.data.len()).await;

            let payload = message.data.trim();
            if payload.is_empty() {
                continue;
            }

            if done_policy.is_done(payload) {
                yield Ok(SseChunk::Done);
                return;
            }

            let limits =
                json_decode_limits(max_frame_bytes, json_value_limits);
            match message.decode_json_with_mode_and_limits::<T>(mode, limits) {
                Ok(Some(data)) => yield Ok(SseChunk::Data(data)),
                Ok(None) => continue,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            }
        }
        if matches!(completion_policy, SseCompletionPolicy::RequireDoneMarker) {
            yield Err(HttpError::sse_protocol("SSE stream ended before its done marker"));
        }
    };

    Box::pin(output)
}

#[cfg(test)]
mod poll_audit_tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::task::Wake;
    use std::task::Waker;

    use bytes::Bytes;
    use futures_util::StreamExt;

    use super::decode_json_chunks_from_stream_with_limits;
    use crate::HttpByteStream;
    use crate::json_limits::default_json_value_limits;
    use crate::sse::DoneMarkerPolicy;
    use crate::sse::SseChunk;
    use crate::sse::SseCompletionPolicy;
    use crate::sse::SseJsonMode;

    struct CountWake(AtomicUsize);

    impl Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_lenient_json_skips_many_invalid_events_with_bounded_polls() {
        const IGNORED: usize = 4_096;
        let consumed = Arc::new(AtomicUsize::new(0));
        let source_count = Arc::clone(&consumed);
        let input_chunks = (0..IGNORED)
            .flat_map(|_| {
                [
                    Bytes::from_static(b"data:\n\n"),
                    Bytes::from_static(b"data: not-json\n\n"),
                ]
            })
            .chain([Bytes::from_static(b"data: {\"value\":1}\n\n")]);
        let total_chunks = IGNORED * 2 + 1;
        let chunks = futures_util::stream::iter(input_chunks).inspect(move |_| {
            source_count.fetch_add(1, Ordering::SeqCst);
        });
        let input: HttpByteStream = Box::pin(chunks.map(Ok));
        let mut output = decode_json_chunks_from_stream_with_limits::<serde_json::Value>(
            input,
            DoneMarkerPolicy::Disabled,
            SseJsonMode::Lenient,
            128,
            256,
            default_json_value_limits(),
            SseCompletionPolicy::AllowEof,
        );

        let wake_counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wake_counter));
        let mut context = std::task::Context::from_waker(&waker);
        let polled = std::future::poll_fn(|_| std::task::Poll::Ready(output.as_mut().poll_next(&mut context))).await;
        assert!(polled.is_pending());
        let first_poll_consumed = consumed.load(Ordering::SeqCst);
        assert_eq!(
            first_poll_consumed, 128,
            "first poll consumed {first_poll_consumed} ready chunks"
        );
        assert!(first_poll_consumed < total_chunks);
        assert!(wake_counter.0.load(Ordering::SeqCst) > 0);

        assert_eq!(
            output.next().await.unwrap().unwrap(),
            SseChunk::Data(serde_json::json!({"value": 1}))
        );
        assert_eq!(consumed.load(Ordering::SeqCst), total_chunks);
    }
}
