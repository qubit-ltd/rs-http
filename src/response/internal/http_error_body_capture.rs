// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error;
use std::sync::Arc;

use bytes::Bytes;

use crate::error::HttpBodyCaptureEnd;

/// Bounded status-body capture and its terminal diagnostic.
pub(in crate::response) struct HttpErrorBodyCapture {
    pub(in crate::response) preview: String,
    pub(in crate::response) raw_body: Bytes,
    pub(in crate::response) end: HttpBodyCaptureEnd,
    pub(in crate::response) error: Option<Arc<dyn Error + Send + Sync + 'static>>,
}
