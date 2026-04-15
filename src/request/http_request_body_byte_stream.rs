/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Pin-boxed byte stream type for streaming request bodies.

use std::pin::Pin;

use bytes::Bytes;
use futures_util::Stream;

/// Pin-boxed async stream used for request streaming upload bodies.
pub type HttpRequestBodyByteStream =
    Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send + 'static>>;
