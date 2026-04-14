/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Request-level retry override options.

use crate::HttpRetryMethodPolicy;

/// Per-request overrides for the client retry strategy.
///
/// This type allows callers to:
/// - force-enable retry even when client-level retry is disabled;
/// - force-disable retry for one request;
/// - override the retryable-method policy for one request;
/// - optionally honor `Retry-After` on `429 Too Many Requests`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequestRetryOverride {
    /// Force retry on for this request (unless explicitly force-disabled).
    force_enable: bool,
    /// Force retry off for this request.
    force_disable: bool,
    /// Optional method-policy override for this request.
    method_policy: Option<HttpRetryMethodPolicy>,
    /// Whether to honor `Retry-After` on `429` responses.
    honor_retry_after: bool,
}

impl Default for HttpRequestRetryOverride {
    fn default() -> Self {
        Self {
            force_enable: false,
            force_disable: false,
            method_policy: None,
            honor_retry_after: false,
        }
    }
}

impl HttpRequestRetryOverride {
    /// Creates an empty override that follows client-level retry settings.
    ///
    /// # Returns
    /// New [`HttpRequestRetryOverride`] with all per-request overrides disabled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables force-retry for this request and clears force-disable.
    ///
    /// # Returns
    /// Updated override for chaining.
    pub fn force_enable(mut self) -> Self {
        self.force_enable = true;
        self.force_disable = false;
        self
    }

    /// Enables force-disable for this request and clears force-enable.
    ///
    /// # Returns
    /// Updated override for chaining.
    pub fn force_disable(mut self) -> Self {
        self.force_disable = true;
        self.force_enable = false;
        self
    }

    /// Sets the per-request retryable-method policy override.
    ///
    /// # Parameters
    /// - `policy`: Method policy to use for this request.
    ///
    /// # Returns
    /// Updated override for chaining.
    pub fn with_method_policy(mut self, policy: HttpRetryMethodPolicy) -> Self {
        self.method_policy = Some(policy);
        self
    }

    /// Enables or disables honoring `Retry-After` for this request.
    ///
    /// # Parameters
    /// - `enabled`: `true` to honor `Retry-After` on `429` responses.
    ///
    /// # Returns
    /// Updated override for chaining.
    pub fn with_honor_retry_after(mut self, enabled: bool) -> Self {
        self.honor_retry_after = enabled;
        self
    }

    /// Returns whether force-enable is active for this request.
    ///
    /// # Returns
    /// `true` when retry is force-enabled at request level.
    pub fn is_force_enable(&self) -> bool {
        self.force_enable
    }

    /// Returns whether force-disable is active for this request.
    ///
    /// # Returns
    /// `true` when retry is force-disabled at request level.
    pub fn is_force_disable(&self) -> bool {
        self.force_disable
    }

    /// Returns the per-request method-policy override, if present.
    ///
    /// # Returns
    /// Optional override for [`HttpRetryMethodPolicy`].
    pub fn method_policy(&self) -> Option<HttpRetryMethodPolicy> {
        self.method_policy
    }

    /// Returns whether `Retry-After` should be honored for this request.
    ///
    /// # Returns
    /// `true` when this request should honor `Retry-After` on `429`.
    pub fn should_honor_retry_after(&self) -> bool {
        self.honor_retry_after
    }

    /// Resolves request-level force-enable/force-disable against client defaults.
    ///
    /// # Parameters
    /// - `client_enabled`: Client-level retry enabled flag.
    ///
    /// # Returns
    /// Effective retry-enabled flag for this request.
    pub(crate) fn resolve_enabled(&self, client_enabled: bool) -> bool {
        if self.force_disable {
            false
        } else if self.force_enable {
            true
        } else {
            client_enabled
        }
    }

    /// Resolves effective method policy from request-level override + client policy.
    ///
    /// # Parameters
    /// - `client_policy`: Client-level method policy.
    ///
    /// # Returns
    /// Effective method policy for this request.
    pub(crate) fn resolve_method_policy(
        &self,
        client_policy: HttpRetryMethodPolicy,
    ) -> HttpRetryMethodPolicy {
        self.method_policy.unwrap_or(client_policy)
    }
}
