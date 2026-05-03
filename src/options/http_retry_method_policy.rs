/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/

use std::str::FromStr;

use http::Method;
use parse_display::FromStr as DeriveFromStr;
use serde::{Deserialize, Serialize};

use super::HttpConfigError;

/// HTTP method policy used to decide whether a request can be retried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, DeriveFromStr)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HttpRetryMethodPolicy {
    /// Retry only HTTP methods that are safe to replay by default.
    #[default]
    #[display("IDEMPOTENT_ONLY")]
    #[from_str(regex = "(?i)IDEMPOTENT_ONLY|IDEMPOTENT")]
    IdempotentOnly,
    /// Retry all HTTP methods, including `POST` and `PATCH`.
    #[display("ALL_METHODS")]
    #[from_str(regex = "(?i)ALL_METHODS|ALL")]
    AllMethods,
    /// Disable method-level retry eligibility.
    #[display("NONE")]
    #[from_str(regex = "(?i)NONE|DISABLED")]
    None,
}

impl HttpRetryMethodPolicy {
    pub(super) fn from_config_value(value: &str) -> Result<Self, HttpConfigError> {
        Self::from_str(value.trim()).map_err(|_| {
            HttpConfigError::invalid_value(
                "method_policy",
                format!("Unsupported retry method policy: {value}"),
            )
        })
    }

    /// Returns whether the retry executor permits replaying `method`.
    ///
    /// # Parameters
    /// - `method`: HTTP method to evaluate.
    ///
    /// # Returns
    /// `true` when automatic retry may replay the method.
    pub fn allows_method(&self, method: &Method) -> bool {
        match self {
            Self::IdempotentOnly => matches!(
                *method,
                Method::GET
                    | Method::HEAD
                    | Method::PUT
                    | Method::DELETE
                    | Method::OPTIONS
                    | Method::TRACE
            ),
            Self::AllMethods => true,
            Self::None => false,
        }
    }
}
