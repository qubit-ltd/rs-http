// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0 (the "License");
// =============================================================================
//! Shared closure-adapter helper for HTTP redaction.

/// Runs one HTTP redaction operation through the session's closure adapter.
macro_rules! http_with {
    ($session:expr, |$adapter:ident| $body:expr) => {{
        let mut result = None;
        let session = $session.http_with(|$adapter| {
            result = Some($body);
        });
        (
            session,
            result.expect("HTTP redaction adapter must run exactly once"),
        )
    }};
}

pub(crate) use http_with;
