// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Defines a sensitive enum fixture for error-redaction tests.

/// Enum whose unknown variants make serde include the input in detailed errors.
#[derive(Debug, serde::Deserialize)]
pub enum SensitiveChoice {
    /// The only accepted test value.
    Allowed,
}
