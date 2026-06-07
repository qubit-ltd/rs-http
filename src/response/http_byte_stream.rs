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

/// Pin-boxed async stream of body chunks or errors, used by
/// [`crate::HttpResponse`].
pub type HttpByteStream = Pin<Box<dyn Stream<Item = HttpResult<Bytes>> + Send>>;
