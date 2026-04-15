/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Byte stream alias for HTTP response body streams.

use std::pin::Pin;

use bytes::Bytes;
use futures_util::Stream;

use crate::HttpResult;

/// Pin-boxed async stream of body chunks or errors, used by [`crate::HttpResponse`].
pub type HttpByteStream = Pin<Box<dyn Stream<Item = HttpResult<Bytes>> + Send>>;
