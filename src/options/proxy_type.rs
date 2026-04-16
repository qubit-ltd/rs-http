/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// URL scheme selector used when constructing the proxy URL for reqwest.
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
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ProxyType {
    /// HTTP proxy.
    #[default]
    #[strum(to_string = "http")]
    Http,
    /// HTTPS proxy.
    #[strum(to_string = "https")]
    Https,
    /// SOCKS5 proxy.
    #[strum(serialize = "socks5h", serialize = "socks5")]
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
