// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Pin-boxed stream of internal SSE records.

use std::pin::Pin;

use futures_util::Stream;

use super::sse_record::SseRecord;
use crate::HttpResult;

/// Pin-boxed stream of internal SSE records or [`HttpError`](crate::HttpError).
pub(crate) type SseRecordStream = Pin<Box<dyn Stream<Item = HttpResult<SseRecord>> + Send>>;
