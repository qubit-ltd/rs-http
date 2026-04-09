/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use qubit_http::ProxyType;

#[test]
fn test_proxy_type_scheme() {
    assert_eq!(ProxyType::Http.scheme(), "http");
    assert_eq!(ProxyType::Https.scheme(), "https");
    assert_eq!(ProxyType::Socks5.scheme(), "socks5h");
}
