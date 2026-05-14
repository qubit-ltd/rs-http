/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use super::BodyLogContext;

/// Bounded body bytes plus enough context to render a log-safe preview.
#[derive(Debug, Clone, Copy)]
pub struct BodyPreview<'a> {
    /// Full body bytes, or an already bounded prefix when `truncated` is set.
    pub(crate) bytes: &'a [u8],
    /// Maximum bytes allowed in the rendered preview.
    pub(crate) limit: usize,
    /// Total source body length when known.
    pub(crate) source_len: usize,
    /// Whether `bytes` is already a truncated prefix.
    pub(crate) truncated: bool,
    /// Optional content type used to choose structured redaction rules.
    pub(crate) content_type: Option<&'a str>,
    /// Logging context that controls truncation marker text.
    pub(crate) context: BodyLogContext,
}

impl<'a> BodyPreview<'a> {
    /// Creates a body preview from source bytes and a byte limit.
    ///
    /// # Parameters
    /// - `bytes`: Source body bytes.
    /// - `limit`: Maximum preview bytes; values below 1 are clamped to 1.
    /// - `context`: Logging call site for truncation marker selection.
    ///
    /// # Returns
    /// Preview descriptor borrowing `bytes`.
    pub fn new(bytes: &'a [u8], limit: usize, context: BodyLogContext) -> Self {
        Self {
            bytes,
            limit: limit.max(1),
            source_len: bytes.len(),
            truncated: false,
            content_type: None,
            context,
        }
    }

    /// Creates a preview from bytes that have already been limited by the caller.
    ///
    /// # Parameters
    /// - `bytes`: Preview prefix bytes.
    /// - `source_len`: Total source body length when known.
    /// - `truncated`: Whether the source had more bytes after `bytes`.
    /// - `context`: Logging call site for truncation marker selection.
    ///
    /// # Returns
    /// Preview descriptor borrowing `bytes`.
    pub(crate) fn from_limited_bytes(
        bytes: &'a [u8],
        source_len: usize,
        truncated: bool,
        context: BodyLogContext,
    ) -> Self {
        Self {
            bytes,
            limit: bytes.len().max(1),
            source_len,
            truncated,
            content_type: None,
            context,
        }
    }

    /// Adds content type metadata used by structured body sanitizers.
    ///
    /// # Parameters
    /// - `content_type`: Content-Type header value.
    ///
    /// # Returns
    /// Updated preview descriptor.
    pub fn with_content_type(mut self, content_type: &'a str) -> Self {
        self.content_type = Some(content_type);
        self
    }

    /// Returns the byte prefix that should be rendered.
    ///
    /// # Returns
    /// Bounded byte slice.
    pub(crate) fn prefix(&self) -> &'a [u8] {
        if self.truncated {
            self.bytes
        } else {
            let end = self.bytes.len().min(self.limit);
            &self.bytes[..end]
        }
    }

    /// Returns whether rendered output is truncated.
    ///
    /// # Returns
    /// `true` when either the caller marked the prefix as truncated or `limit`
    /// cuts the provided source bytes.
    pub(crate) fn is_truncated(&self) -> bool {
        self.truncated || self.bytes.len() > self.limit
    }

    /// Returns the total source body length used for binary previews.
    ///
    /// # Returns
    /// Source body length in bytes.
    pub(crate) fn source_len(&self) -> usize {
        self.source_len.max(self.bytes.len())
    }

    /// Returns the truncation suffix for this preview.
    ///
    /// # Returns
    /// Empty string when not truncated.
    pub(crate) fn truncation_suffix(&self) -> String {
        if !self.is_truncated() {
            return String::new();
        }
        match self.context {
            BodyLogContext::ErrorResponse => "...<truncated>".to_string(),
            BodyLogContext::Request | BodyLogContext::Response => {
                let truncated = self.source_len().saturating_sub(self.prefix().len());
                format!("...<truncated {truncated} bytes>")
            }
        }
    }
}
