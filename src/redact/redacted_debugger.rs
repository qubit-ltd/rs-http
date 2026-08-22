// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
//! Safe rendering helpers for `Debug` implementations.

use qubit_redact::RedactedText;
use qubit_redact::Redactor;
use url::Url;

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
    #[inline(always)]
    pub(crate) const fn new(log_redactor: &'redactor Redactor) -> Self {
        Self {
            redactor: log_redactor,
        }
    }

    /// Returns the immutable redactor used for each independent field.
    #[inline(always)]
    pub(crate) const fn redactor(&self) -> &'redactor Redactor {
        self.redactor
    }

    /// Returns an optional redacted URL through the direct adapter API.
    #[inline(always)]
    pub(crate) fn optional_url(
        &self,
        url: Option<&Url>,
    ) -> Option<RedactedText> {
        url.map(|url| self.redactor.redact_http_url(url.as_str()).into_text())
    }

    /// Redacts HTTP URLs embedded in a diagnostic message.
    pub(crate) fn urls_in_text(&self, text: &str) -> String {
        let mut output = String::with_capacity(text.len());
        let mut cursor = 0;
        while let Some(relative_start) = find_url_start(&text[cursor..]) {
            let start = cursor + relative_start;
            output.push_str(&text[cursor..start]);
            let next_search_start = start + 1;
            let candidate_limit = find_url_start(&text[next_search_start..])
                .map_or(text.len(), |next| next_search_start + next);
            let candidate_limit = text[start..candidate_limit]
                .char_indices()
                .find_map(|(index, character)| {
                    character.is_whitespace().then_some(start + index)
                })
                .unwrap_or(candidate_limit);
            let end = url_candidate_end(&text[..candidate_limit], start);
            let candidate = &text[start..end];
            output.push_str(
                Url::parse(candidate)
                    .map_or_else(
                        |_| "<redacted: invalid URL>".to_owned(),
                        |url| {
                            self.redactor
                                .redact_http_url(url.as_str())
                                .into_text()
                                .into_string()
                        },
                    )
                    .as_str(),
            );
            cursor = end;
        }
        output.push_str(&text[cursor..]);
        output
    }
}

fn find_url_start(text: &str) -> Option<usize> {
    let find = |needle: &str| {
        text.as_bytes()
            .windows(needle.len())
            .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
    };
    match (find("http://"), find("https://")) {
        (Some(http), Some(https)) => Some(http.min(https)),
        (Some(index), None) | (None, Some(index)) => Some(index),
        (None, None) => None,
    }
}

fn url_candidate_end(text: &str, start: usize) -> usize {
    let mut end = text.len();
    while let Some((index, character)) = text[..end].char_indices().next_back()
    {
        if index <= start
            || !matches!(
                character,
                '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}'
            )
        {
            break;
        }
        end = index;
    }
    end
}
