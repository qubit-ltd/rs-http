// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::str::FromStr;

use qubit_http::ProxyType;

#[test]
fn test_proxy_type_scheme() {
    assert_eq!(ProxyType::Http.scheme(), "http");
    assert_eq!(ProxyType::Https.scheme(), "https");
    assert_eq!(ProxyType::Socks5.scheme(), "socks5h");
}

#[test]
fn test_proxy_type_from_str_is_case_insensitive() {
    assert_eq!(ProxyType::from_str("HTTP").expect("parse http"), ProxyType::Http);
    assert_eq!(
        ProxyType::from_str("socks5h").expect("parse socks5h"),
        ProxyType::Socks5
    );
}

#[test]
fn test_proxy_type_from_str_invalid_value() {
    assert!(ProxyType::from_str("ftp").is_err());
}
