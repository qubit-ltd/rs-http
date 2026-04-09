/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Pin-boxed stream of [`SseEvent`](crate::sse::SseEvent) or [`HttpError`](crate::HttpError).
//!
//! # Author
//!
//! Haixing Hu

use std::pin::Pin;

use futures_util::Stream;

use crate::HttpResult;

use super::sse_event::SseEvent;

/// Pin-boxed stream of parsed [`SseEvent`] or [`HttpError`](crate::HttpError).
pub type SseEventStream = Pin<Box<dyn Stream<Item = HttpResult<SseEvent>> + Send>>;
