// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

mod internal;

use std::time::Duration;

use internal::tls_tunnel::TlsTunnel;
use reqwest::Certificate;
use tokio::time::timeout;

use crate::HttpClient;
use crate::HttpClientOptions;
use crate::client::http_client_builder::HttpClientBuilder;

const TEST_TIMEOUT: Duration = Duration::from_secs(8);

fn options(proxy_addr: std::net::SocketAddr) -> HttpClientOptions {
    let mut options = HttpClientOptions::default();
    options.proxy.enabled = true;
    options.proxy.host = Some(proxy_addr.ip().to_string());
    options.proxy.port = Some(proxy_addr.port());
    options.proxy.username = Some("tunnel-user".to_owned());
    options.proxy.password = Some("tunnel-password".to_owned());
    options
}

fn configured_client(options: HttpClientOptions, trust_test_ca: bool) -> HttpClient {
    let mut builder = HttpClientBuilder::configure_backend(&options).expect("production backend configuration");
    if trust_test_ca {
        let ca = Certificate::from_der(include_bytes!("../../../tests/fixtures/tls/ca.der"))
            .expect("valid test CA certificate");
        builder = builder.add_root_certificate(ca);
    }
    HttpClient::new(builder.build().expect("reqwest client builds"), options)
}

#[tokio::test]
async fn test_https_connect_tunnel_success() {
    let tunnel = timeout(TEST_TIMEOUT, TlsTunnel::start())
        .await
        .expect("TLS tunnel service startup timeout")
        .expect("start local TLS and CONNECT services");
    let tls_port = tunnel.tls_port();
    let client = configured_client(options(tunnel.proxy_addr()), true);
    let request = client
        .request(
            http::Method::GET,
            &format!("https://localhost:{tls_port}/through-proxy"),
        )
        .header("x-business-request", "tunnel-test")
        .expect("valid header")
        .build();

    let mut response = timeout(TEST_TIMEOUT, client.execute(request))
        .await
        .expect("client request completes")
        .expect("TLS over CONNECT succeeds");
    assert_eq!(response.status(), http::StatusCode::OK);
    let body = timeout(TEST_TIMEOUT, response.text())
        .await
        .expect("response body read timeout")
        .expect("read response body");
    assert_eq!(body, "tls tunnel response body");

    let observation = timeout(TEST_TIMEOUT, tunnel.finish())
        .await
        .expect("service tasks finish");
    assert_eq!(observation.connect_authority, format!("localhost:{tls_port}"));
    assert_eq!(
        observation.proxy_authorization.as_deref(),
        Some("Basic dHVubmVsLXVzZXI6dHVubmVsLXBhc3N3b3Jk")
    );
    assert!(observation.tls_handshake_succeeded);
    let request = observation
        .application_request
        .expect("TLS server received application request");
    assert!(request.starts_with("GET /through-proxy HTTP/1.1\r\n"), "{request:?}");
    assert!(
        request
            .to_ascii_lowercase()
            .contains("x-business-request: tunnel-test\r\n"),
        "{request:?}"
    );
    assert!(
        !request.to_ascii_lowercase().contains("proxy-authorization:"),
        "{request:?}"
    );
}

#[tokio::test]
async fn test_https_connect_tunnel_rejects_untrusted_ca() {
    let tunnel = timeout(TEST_TIMEOUT, TlsTunnel::start())
        .await
        .expect("TLS tunnel service startup timeout")
        .expect("start local TLS and CONNECT services");
    let client = configured_client(options(tunnel.proxy_addr()), false);
    let request = client
        .request(
            http::Method::GET,
            &format!("https://localhost:{}/untrusted", tunnel.tls_port()),
        )
        .build();

    let result = timeout(TEST_TIMEOUT, client.execute(request))
        .await
        .expect("failed TLS request completes");
    assert!(result.is_err(), "untrusted test CA unexpectedly succeeded");

    let observation = timeout(TEST_TIMEOUT, tunnel.finish())
        .await
        .expect("service tasks finish after TLS rejection");
    assert!(observation.connect_authority.starts_with("localhost:"));
    assert!(
        !observation.tls_handshake_succeeded,
        "TLS handshake unexpectedly completed"
    );
    assert!(
        observation.application_request.is_none(),
        "TLS server received an HTTP application request"
    );
}
