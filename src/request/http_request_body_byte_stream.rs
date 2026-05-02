/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! Pin-boxed byte stream type for streaming request bodies.

use std::pin::Pin;

use bytes::Bytes;
use futures_util::Stream;

/// Pin-boxed async stream used for request streaming upload bodies.
pub type HttpRequestBodyByteStream =
    Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send + 'static>>;
