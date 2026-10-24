// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Pin-boxed byte stream type for streaming request bodies.

use std::io::Result as IoResult;
use std::pin::Pin;

use bytes::Bytes;
use futures_util::Stream;

/// A pinned, boxed stream of byte chunks for a streaming request body.
///
/// Each item contains one chunk or an I/O error. The stream can be moved
/// between threads and must own all data it references because it is `'static`.
pub type HttpRequestBodyByteStream = Pin<Box<dyn Stream<Item = IoResult<Bytes>> + Send + 'static>>;
