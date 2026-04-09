/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
//! Header injector abstraction for outgoing requests.

use http::HeaderMap;

use crate::HttpResult;

/// Hook for adding or mutating headers on every outgoing request (e.g. auth tokens).
pub trait HeaderInjector: Send + Sync {
    /// Merges injector-specific headers into the map before per-request headers are applied.
    ///
    /// # Parameters
    /// - `headers`: Map to mutate (already contains default client headers).
    ///
    /// # Returns
    /// `Ok(())` or [`crate::HttpError`] if injection fails (e.g. invalid value).
    fn inject(&self, headers: &mut HeaderMap) -> HttpResult<()>;
}
