// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Decode public SSE messages from an HTTP body byte stream with size limits.

use async_stream::stream;
use futures_util::StreamExt;

use super::SseMessageStream;
use super::SseRecord;
use super::SseRecordStream;
use super::frame_decoder;
use super::internal::decode_poll_budget::DecodePollBudget;
use super::line_decoder;
use crate::HttpByteStream;

/// Parses internal SSE records from a body byte stream with explicit size
/// limits.
///
/// # Parameters
/// - `stream`: Body byte stream whose payload is SSE text.
/// - `max_line_bytes`: Maximum allowed bytes for one SSE line.
/// - `max_frame_bytes`: Maximum allowed bytes for one SSE frame.
///
/// # Returns
/// Stream yielding internal [`SseRecord`] values or protocol/transport errors.
#[must_use]
pub(crate) fn decode_records_from_stream_with_limits(
    stream: HttpByteStream,
    max_line_bytes: usize,
    max_frame_bytes: usize,
) -> SseRecordStream {
    let lines = line_decoder::decode_lines(stream, max_line_bytes);
    frame_decoder::decode_records(lines, max_frame_bytes)
}

/// Parses public SSE messages from a body byte stream with explicit size
/// limits.
///
/// # Parameters
/// - `stream`: Body byte stream whose payload is SSE text.
/// - `max_line_bytes`: Maximum allowed bytes for one SSE line.
/// - `max_frame_bytes`: Maximum allowed bytes for one SSE frame.
///
/// # Returns
/// Stream yielding [`super::SseMessage`] values or protocol/transport errors.
#[must_use]
pub(crate) fn decode_messages_from_stream_with_limits(
    stream: HttpByteStream,
    max_line_bytes: usize,
    max_frame_bytes: usize,
) -> SseMessageStream {
    let mut records = decode_records_from_stream_with_limits(stream, max_line_bytes, max_frame_bytes);
    let output = stream! {
        let mut poll_budget = DecodePollBudget::default();
        while let Some(item) = records.next().await {
            let bytes = item.as_ref().map_or(0, |record| match record {
                SseRecord::Dispatch(message) => message.data.len(),
                SseRecord::Control(_) => 0,
            });
            poll_budget.account(1, bytes).await;
            match item {
                Ok(SseRecord::Dispatch(message)) => yield Ok(message),
                Ok(SseRecord::Control(_)) => continue,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            }
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

    use super::decode_messages_from_stream_with_limits;
    use crate::HttpByteStream;

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
    async fn test_immediately_ready_sse_input_yields_with_bounded_outer_poll_work() {
        const HEARTBEATS: usize = 4_096;
        let side_task_ran = Arc::new(AtomicUsize::new(0));
        let side_task_flag = Arc::clone(&side_task_ran);
        tokio::spawn(async move {
            side_task_flag.store(1, Ordering::SeqCst);
        });
        let task_before_final_chunk = Arc::new(AtomicUsize::new(0));
        let task_observed = Arc::clone(&task_before_final_chunk);
        let side_task_observed = Arc::clone(&side_task_ran);
        let consumed = Arc::new(AtomicUsize::new(0));
        let source_count = Arc::clone(&consumed);
        let mut input_chunks = Vec::with_capacity(HEARTBEATS * 2 + 1);
        for _ in 0..HEARTBEATS {
            input_chunks.push(Bytes::from_static(b": heartbeat\n\n"));
            input_chunks.push(Bytes::from_static(b"id: cursor\nretry: 17\n\n"));
        }
        input_chunks.push(Bytes::from_static(b"data: ok\n\n"));
        let chunks = futures_util::stream::iter(input_chunks).inspect(move |chunk| {
            source_count.fetch_add(1, Ordering::SeqCst);
            if chunk.as_ref() == b"data: ok\n\n" {
                task_observed.store(side_task_observed.load(Ordering::SeqCst), Ordering::SeqCst);
            }
        });
        let input: HttpByteStream = Box::pin(chunks.map(Ok));
        let mut output = decode_messages_from_stream_with_limits(input, 128, 256);

        let wake_counter = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wake_counter));
        let mut context = std::task::Context::from_waker(&waker);
        let polled = std::future::poll_fn(|_| {
            // Poll the combined decoder explicitly once and return that result.
            std::task::Poll::Ready(output.as_mut().poll_next(&mut context))
        })
        .await;

        assert!(polled.is_pending(), "decoder should yield before draining the input");
        let first_poll_consumed = consumed.load(Ordering::SeqCst);
        assert!(
            (1..=1024).contains(&first_poll_consumed),
            "first poll consumed {first_poll_consumed} ready chunks"
        );
        assert!(first_poll_consumed < HEARTBEATS * 2 + 1);
        assert!(wake_counter.0.load(Ordering::SeqCst) > 0);
        assert_eq!(side_task_ran.load(Ordering::SeqCst), 0);

        let message = output.next().await.unwrap().unwrap();
        assert_eq!(message.data, "ok");
        assert_eq!(consumed.load(Ordering::SeqCst), HEARTBEATS * 2 + 1);
        assert_eq!(side_task_ran.load(Ordering::SeqCst), 1);
        assert_eq!(task_before_final_chunk.load(Ordering::SeqCst), 1);
    }
}
