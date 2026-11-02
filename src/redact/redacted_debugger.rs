// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Safe rendering helpers for `Debug` implementations.

use qubit_redact::RedactedText;
use qubit_redact::Redactor;
use url::Url;

use crate::redact::internal::MessageEmitter;
use crate::redact::internal::MessageSegment;

/// Renders diagnostic fields with one immutable policy snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RedactedDebugger<'redactor> {
    /// Unified redactor used for every debug field.
    redactor: &'redactor Redactor,
}

impl<'redactor> RedactedDebugger<'redactor> {
    /// Creates a debugger from the supplied policy without merging defaults.
    ///
    /// # Parameters
    ///
    /// * `log_redactor` - Immutable redactor snapshot to use.
    ///
    /// # Returns
    ///
    /// A safe debug renderer.
    #[must_use]
    #[inline]
    pub(crate) const fn new(log_redactor: &'redactor Redactor) -> Self {
        Self { redactor: log_redactor }
    }

    /// Returns an optional redacted URL through the direct adapter API.
    ///
    /// # Returns
    ///
    /// `Some` contains the redacted URL when `url` is present; `None` means no
    /// URL was supplied.
    pub(crate) fn optional_url(&self, url: Option<&Url>) -> Option<RedactedText> {
        url.map(|url| {
            self.redactor
                .redact_http_url(url.as_str())
                .into_text_or_marker("<redaction incomplete>")
        })
    }

    /// Redacts HTTP URLs embedded in a diagnostic message.
    ///
    /// URL candidates end at whitespace or at the next embedded HTTP scheme;
    /// trailing `.`, `,`, `;`, `:`, `!`, `?`, `)`, `]`, and `}` are left in the
    /// surrounding plain text. Plain control and directional formatting
    /// characters are escaped. The output is limited by bytes, not characters;
    /// if the message cannot be rendered within that budget, this returns the
    /// complete `<redaction incomplete>` marker when it fits, or an empty
    /// string otherwise.
    ///
    /// # Returns
    ///
    /// The message with each HTTP URL replaced by its redacted representation.
    #[must_use]
    pub(crate) fn urls_in_text(&self, text: &str) -> String {
        const MARKER: &str = "<redaction incomplete>";
        let limits = self.redactor.policy().limits();
        let output_limit = limits.max_output_bytes();
        if output_limit == 0 {
            return String::new();
        }
        if text.len() > limits.max_input_bytes() {
            return bounded_marker(output_limit);
        }

        let mut batch = self.redactor.diagnostic_batch();
        let mut segments = Vec::new();
        let mut cursor = 0;
        let mut incomplete_tail = false;
        while let Some((start, _scheme_len)) = find_url_start(text, cursor) {
            if start > cursor {
                segments.push(MessageSegment::Plain(cursor..start));
            }
            let candidate_end = url_candidate_limit(text, start);
            let end = url_candidate_end(text, start, candidate_end);
            if end == start {
                cursor = start + 1;
                continue;
            }
            let handle = batch.redact_http_url(&text[start..end]);
            segments.push(MessageSegment::Url(handle));
            cursor = end;
            if batch.is_output_exhausted() {
                incomplete_tail = cursor < text.len();
                break;
            }
        }
        if !incomplete_tail && cursor < text.len() {
            segments.push(MessageSegment::Plain(cursor..text.len()));
        }
        let diagnostics = batch.finish_with_marker(MARKER);
        let mut emitter = MessageEmitter::new(output_limit);
        for segment in segments {
            let complete = match segment {
                MessageSegment::Plain(range) => emitter.push_plain(&text[range]),
                MessageSegment::Url(handle) => emitter.push_safe(diagnostics.text(handle).as_str()),
            };
            if !complete {
                return bounded_marker(output_limit);
            }
        }
        if incomplete_tail && !emitter.push_safe(MARKER) {
            return bounded_marker(output_limit);
        }
        emitter.finish()
    }
}

/// Finds a scheme at or after `cursor`, inspecting each byte at most once.
fn find_url_start(text: &str, cursor: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut index = cursor;
    while index + 7 <= bytes.len() {
        let bytes = &bytes[index..];
        if bytes[..7].eq_ignore_ascii_case(b"http://") {
            return Some((index, 7));
        }
        if bytes.len() >= 8 && bytes[..8].eq_ignore_ascii_case(b"https://") {
            return Some((index, 8));
        }
        index += 1;
    }
    None
}

/// Finds the byte limit of an embedded HTTP URL candidate.
///
/// # Parameters
/// - `text`: Diagnostic text containing the recognized HTTP scheme.
/// - `start`: Byte offset of the ASCII h at that scheme's start.
///
/// # Returns
/// The next whitespace or HTTP scheme offset, or the input length. Scanning
/// advances by UTF-8 characters so the result remains a string boundary.
fn url_candidate_limit(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut index = start + 1;
    while index < bytes.len() {
        let character = text[index..].chars().next().expect("index remains on a UTF-8 boundary");
        if character.is_whitespace() {
            return index;
        }
        if bytes[index..].len() >= 7
            && (bytes[index..index + 7].eq_ignore_ascii_case(b"http://")
                || (bytes[index..].len() >= 8 && bytes[index..index + 8].eq_ignore_ascii_case(b"https://")))
        {
            return index;
        }
        index += character.len_utf8();
    }
    text.len()
}

/// Removes selected trailing punctuation from a bounded URL candidate.
///
/// # Parameters
/// - `text`: Diagnostic text containing the candidate.
/// - `start`: Candidate start at a valid UTF-8 boundary.
/// - `limit`: Candidate limit at a valid UTF-8 boundary, not before start.
///
/// # Returns
/// The UTF-8 boundary after stripping trailing . , ; : ! ? ) ] and }.
/// The caller renders those punctuation characters as surrounding plain text;
/// this helper does not validate URL syntax.
fn url_candidate_end(text: &str, start: usize, limit: usize) -> usize {
    let mut end = limit;
    while let Some((index, character)) = text[start..end].char_indices().next_back() {
        if !matches!(character, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}') {
            break;
        }
        end = start + index;
    }
    end
}

/// Returns the full marker when it fits the byte budget, and an empty string
/// otherwise.
fn bounded_marker(maximum: usize) -> String {
    const MARKER: &str = "<redaction incomplete>";
    if MARKER.len() <= maximum {
        MARKER.to_owned()
    } else {
        String::new()
    }
}
