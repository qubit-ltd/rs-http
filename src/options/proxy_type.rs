/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

/// URL scheme selector used when constructing the proxy URL for reqwest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxyType {
    /// HTTP proxy.
    #[default]
    Http,
    /// HTTPS proxy.
    Https,
    /// SOCKS5 proxy.
    Socks5,
}

impl ProxyType {
    /// Returns the URL scheme string embedded in `proxy_url` for reqwest.
    ///
    /// # Parameters
    /// - `self`: Proxy kind.
    ///
    /// # Returns
    /// `"http"`, `"https"`, or `"socks5h"` (SOCKS5 with remote DNS).
    pub fn scheme(self) -> &'static str {
        match self {
            ProxyType::Http => "http",
            ProxyType::Https => "https",
            ProxyType::Socks5 => "socks5h",
        }
    }
}
