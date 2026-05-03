/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Pin-boxed stream of [`SseEvent`](crate::sse::SseEvent) or [`HttpError`](crate::HttpError).
//!

use std::pin::Pin;

use futures_util::Stream;

use crate::HttpResult;

use super::sse_event::SseEvent;

/// Pin-boxed stream of parsed [`SseEvent`] or [`HttpError`](crate::HttpError).
pub type SseEventStream = Pin<Box<dyn Stream<Item = HttpResult<SseEvent>> + Send>>;
