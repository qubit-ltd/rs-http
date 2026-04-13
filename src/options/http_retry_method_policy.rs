/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use http::Method;

use super::HttpConfigError;

/// HTTP method policy used to decide whether a request can be retried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpRetryMethodPolicy {
    /// Retry only HTTP methods that are safe to replay by default.
    IdempotentOnly,
    /// Retry all HTTP methods, including `POST` and `PATCH`.
    AllMethods,
    /// Disable method-level retry eligibility.
    None,
}

impl Default for HttpRetryMethodPolicy {
    fn default() -> Self {
        Self::IdempotentOnly
    }
}

impl HttpRetryMethodPolicy {
    pub(super) fn from_config_value(value: &str) -> Result<Self, HttpConfigError> {
        let normalized = value.trim().to_ascii_uppercase().replace('-', "_");
        match normalized.as_str() {
            "IDEMPOTENT_ONLY" | "IDEMPOTENT" => Ok(Self::IdempotentOnly),
            "ALL_METHODS" | "ALL" => Ok(Self::AllMethods),
            "NONE" | "DISABLED" => Ok(Self::None),
            _ => Err(HttpConfigError::invalid_value(
                "method_policy",
                format!("Unsupported retry method policy: {value}"),
            )),
        }
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
