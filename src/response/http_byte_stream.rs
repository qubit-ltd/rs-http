// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Byte stream alias for HTTP response body streams.

use std::pin::Pin;

use bytes::Bytes;
use futures_util::Stream;

use crate::HttpResult;

/// Owned, pinned stream of HTTP response body chunks, used by
/// [`crate::HttpResponse`].
///
/// Each item yields one [`Bytes`] chunk or an [`HttpError`](crate::HttpError).
/// The stream is `Send`, so callers can move it between threads, and polling
/// it drives consumption of the response body.
pub type HttpByteStream = Pin<Box<dyn Stream<Item = HttpResult<Bytes>> + Send>>;
