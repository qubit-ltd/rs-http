// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stream type for [`SseChunk`](crate::sse::SseChunk) items.

use std::pin::Pin;

use futures_util::Stream;

use super::sse_chunk::SseChunk;
use crate::HttpResult;

/// A pinned, boxed stream of decoded server-sent event chunks.
///
/// Each successful item is an [`SseChunk<T>`], which is either a decoded data
/// value or an explicit end marker. Stream failures are reported as
/// [`HttpError`](crate::HttpError) values through
/// [`HttpResult`](crate::HttpResult). The stream is `Send`, so it can be moved
/// between threads when its captured state satisfies the same requirement.
///
/// # Examples
///
/// ```
/// use futures_util::StreamExt;
/// use futures_util::stream::iter;
/// use qubit_http::sse::SseChunk;
/// use qubit_http::sse::SseChunkStream;
///
/// async fn read_chunks() {
///     let mut chunks: SseChunkStream<u32> = Box::pin(iter([
///         Ok(SseChunk::Data(7)),
///         Ok(SseChunk::Done),
///     ]));
///
///     assert_eq!(chunks.next().await.unwrap().unwrap(), SseChunk::Data(7));
///     assert_eq!(chunks.next().await.unwrap().unwrap(), SseChunk::Done);
/// }
/// ```
///
/// # Type Parameters
///
/// * `T` - The type produced by decoding each SSE data payload.
pub type SseChunkStream<T> = Pin<Box<dyn Stream<Item = HttpResult<SseChunk<T>>> + Send>>;
