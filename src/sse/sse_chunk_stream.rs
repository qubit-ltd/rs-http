/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Stream type for [`SseChunk`](crate::sse::SseChunk) items.
//!
//! # Author
//!
//! Haixing Hu

use std::pin::Pin;

use futures_util::Stream;

use crate::HttpResult;

use super::sse_chunk::SseChunk;

/// Pin-boxed stream of [`SseChunk`] or [`HttpError`](crate::HttpError).
pub type SseChunkStream<T> = Pin<Box<dyn Stream<Item = HttpResult<SseChunk<T>>> + Send>>;
