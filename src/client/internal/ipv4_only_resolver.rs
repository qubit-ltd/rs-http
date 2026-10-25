// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! DNS resolver used by HTTP clients configured for IPv4-only lookup.

use std::net::SocketAddr;

use qubit_error::BoxError;
use qubit_error::IntoBoxError;
use reqwest::dns::Addrs;
use reqwest::dns::Name;
use reqwest::dns::Resolve;
use reqwest::dns::Resolving;

/// DNS resolver that filters out non-IPv4 addresses for `ipv4_only` mode.
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::client) struct Ipv4OnlyResolver;

impl Resolve for Ipv4OnlyResolver {
    /// Resolves `name` using the OS resolver and returns only IPv4 addresses.
    ///
    /// # Parameters
    /// - `name`: DNS name to resolve.
    ///
    /// # Returns
    /// A future yielding only IPv4 socket addresses.
    ///
    /// # Errors
    /// Returns an error when DNS lookup fails or when no IPv4 address exists
    /// for `name`.
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let resolved = match tokio::net::lookup_host((host.as_str(), 0)).await {
                Ok(resolved) => resolved,
                Err(error) => return Err(error.into_box_error()),
            };
            filter_ipv4_addrs(&host, resolved)
        })
    }
}

/// Filters resolved socket addresses down to IPv4 addresses.
///
/// # Parameters
/// - `host`: Hostname used for diagnostics when no IPv4 address remains.
/// - `resolved`: Iterator of resolved socket addresses.
///
/// # Returns
/// Boxed reqwest DNS address iterator containing only IPv4 addresses.
///
/// # Errors
/// Returns an [`std::io::ErrorKind::AddrNotAvailable`] error when resolution
/// produced no IPv4 address.
fn filter_ipv4_addrs<I>(host: &str, resolved: I) -> Result<Addrs, BoxError>
where
    I: IntoIterator<Item = SocketAddr>,
{
    let ipv4_addrs: Vec<SocketAddr> = resolved.into_iter().filter(SocketAddr::is_ipv4).collect();
    if ipv4_addrs.is_empty() {
        let error = std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            format!("No IPv4 address found for host '{host}'"),
        );
        return Err(error.into_box_error());
    }
    Ok(Box::new(ipv4_addrs.into_iter()) as Addrs)
}
