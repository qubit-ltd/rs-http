/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::str::FromStr;

use http::Method;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

use super::HttpConfigError;

/// HTTP method policy used to decide whether a request can be retried.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    Display,
    EnumString,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(ascii_case_insensitive)]
pub enum HttpRetryMethodPolicy {
    /// Retry only HTTP methods that are safe to replay by default.
    #[default]
    #[strum(serialize = "IDEMPOTENT_ONLY", serialize = "IDEMPOTENT")]
    IdempotentOnly,
    /// Retry all HTTP methods, including `POST` and `PATCH`.
    #[strum(serialize = "ALL_METHODS", serialize = "ALL")]
    AllMethods,
    /// Disable method-level retry eligibility.
    #[strum(serialize = "NONE", serialize = "DISABLED")]
    None,
}

impl HttpRetryMethodPolicy {
    pub(super) fn from_config_value(value: &str) -> Result<Self, HttpConfigError> {
        let normalized = value.trim().to_ascii_uppercase().replace('-', "_");
        Self::from_str(&normalized).map_err(|_| {
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
