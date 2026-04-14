/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Streaming response body payload.

use std::fmt;

use crate::HttpByteStream;

use crate::SseDecodeOptions;

/// Streaming response payload plus SSE decode options.
pub struct StreamingBody {
    /// Body as an async stream of [`bytes::Bytes`] chunks.
    pub stream: HttpByteStream,
    /// Default SSE decode options for this stream.
    pub sse_decode_options: SseDecodeOptions,
}

impl fmt::Debug for StreamingBody {
    /// Debug output intentionally omits stream internals.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamingBody")
            .field("sse_decode_options", &self.sse_decode_options)
            .finish_non_exhaustive()
    }
}
