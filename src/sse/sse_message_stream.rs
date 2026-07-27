// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Pin-boxed stream of [`SseMessage`](crate::sse::SseMessage) values.

use std::pin::Pin;

use futures_util::Stream;

use crate::HttpResult;

use super::sse_message::SseMessage;

/// Pin-boxed stream of parsed [`SseMessage`] values or
/// [`HttpError`](crate::HttpError).
pub type SseMessageStream = Pin<Box<dyn Stream<Item = HttpResult<SseMessage>> + Send>>;
