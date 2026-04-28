/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use parse_display::FromStr as DeriveFromStr;
use serde::{Deserialize, Serialize};

/// URL scheme selector used when constructing the proxy URL for reqwest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, DeriveFromStr)]
#[serde(rename_all = "snake_case")]
#[display(style = "snake_case")]
pub enum ProxyType {
    /// HTTP proxy.
    #[default]
    #[display("http")]
    #[from_str(regex = "(?i)http")]
    Http,
    /// HTTPS proxy.
    #[display("https")]
    #[from_str(regex = "(?i)https")]
    Https,
    /// SOCKS5 proxy.
    #[display("socks5h")]
    #[from_str(regex = "(?i)socks5h|socks5")]
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
