// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use qubit_http::HttpClient;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use tokio::time::timeout;

use crate::common::tls_tunnel::TlsTunnel;

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

fn trusted_options() -> HttpClientOptions {
    let mut options = HttpClientOptions::default();
    options
        .extra_root_certificates_der
        .push(bytes::Bytes::from_static(include_bytes!(
            "../fixtures/tls/ca.der"
        )));
    options
}

async fn tls_client_request(
    client: &HttpClient,
    host: &str,
    port: u16,
) -> Result<String, qubit_http::HttpError> {
    let request = client
        .request(
            http::Method::GET,
            &format!("https://{host}:{port}/through-proxy"),
        )
        .header("x-business-request", "tunnel-test")
        .expect("valid header")
        .build();
    let mut response = timeout(TEST_TIMEOUT, client.execute(request))
        .await
        .expect("client request timeout")?;
    timeout(TEST_TIMEOUT, response.text())
        .await
        .expect("response body read timeout")
}

#[tokio::test]
async fn test_tls_roots_public_options_trust_private_ca_through_connect() {
    let tunnel = timeout(TEST_TIMEOUT, TlsTunnel::start())
        .await
        .expect("TLS tunnel startup timeout")
        .expect("start TLS tunnel");
    let tls_port = tunnel.tls_port();
    let mut options = options(tunnel.proxy_addr());
    options
        .extra_root_certificates_der
        .push(bytes::Bytes::from_static(include_bytes!(
            "../fixtures/tls/ca.der"
        )));
    let client = HttpClient::builder()
        .options(options)
        .build()
        .expect("public client build");
    assert_eq!(
        tls_client_request(&client, "localhost", tls_port)
            .await
            .expect("trusted TLS response"),
        "tls tunnel response body"
    );
    let observation = timeout(TEST_TIMEOUT, tunnel.finish())
        .await
        .expect("server finish timeout");
    assert_eq!(
        observation.connect_authority,
        format!("localhost:{tls_port}")
    );
    assert_eq!(
        observation.proxy_authorization.as_deref(),
        Some("Basic dHVubmVsLXVzZXI6dHVubmVsLXBhc3N3b3Jk")
    );
    assert!(observation.tls_handshake_succeeded);
    let request = observation
        .application_request
        .expect("application request received");
    assert!(
        request.starts_with("GET /through-proxy HTTP/1.1\r\n"),
        "{request:?}"
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains("x-business-request: tunnel-test\r\n"),
        "{request:?}"
    );
    assert!(
        !request
            .to_ascii_lowercase()
            .contains("proxy-authorization:"),
        "{request:?}"
    );
}

#[tokio::test]
async fn test_tls_roots_default_roots_reject_private_ca() {
    let tunnel = timeout(TEST_TIMEOUT, TlsTunnel::start())
        .await
        .expect("startup timeout")
        .expect("start tunnel");
    let client = HttpClient::builder()
        .options(options(tunnel.proxy_addr()))
        .build()
        .expect("client build");
    let error = tls_client_request(&client, "localhost", tunnel.tls_port())
        .await
        .expect_err("private CA must be rejected");
    assert!(matches!(
        error.kind(),
        HttpErrorKind::Transport | HttpErrorKind::Other
    ));
    let observation = timeout(TEST_TIMEOUT, tunnel.finish())
        .await
        .expect("server finish timeout");
    assert!(!observation.tls_handshake_succeeded);
    assert!(observation.application_request.is_none());
}

#[tokio::test]
async fn test_tls_roots_still_reject_hostname_mismatch() {
    let tunnel = timeout(TEST_TIMEOUT, TlsTunnel::start())
        .await
        .expect("startup timeout")
        .expect("start tunnel");
    let mut opts = options(tunnel.proxy_addr());
    opts.extra_root_certificates_der = trusted_options().extra_root_certificates_der;
    let client = HttpClient::builder()
        .options(opts)
        .build()
        .expect("client build");
    let error = tls_client_request(&client, "127.0.0.1", tunnel.tls_port())
        .await
        .expect_err("hostname mismatch must fail");
    assert!(matches!(
        error.kind(),
        HttpErrorKind::Transport | HttpErrorKind::Other
    ));
    let observation = timeout(TEST_TIMEOUT, tunnel.finish())
        .await
        .expect("server finish timeout");
    assert!(!observation.tls_handshake_succeeded);
    assert!(observation.application_request.is_none());
}

#[test]
fn test_tls_roots_bad_and_empty_der_fail_build_without_echoing_bytes() {
    for der in [
        bytes::Bytes::from_static(b"not a DER certificate"),
        bytes::Bytes::new(),
    ] {
        let marker = format!("{der:?}");
        let mut options = HttpClientOptions::default();
        options.extra_root_certificates_der.push(der);
        let error = HttpClient::builder()
            .options(options)
            .build()
            .expect_err("invalid DER must fail build");
        assert!(matches!(
            error.kind(),
            HttpErrorKind::BuildClient | HttpErrorKind::Other
        ));
        assert!(
            error.message().contains("additional trust roots")
                || error.message().contains("extra_root_certificates_der[0]")
        );
        assert!(!error.message().contains(&marker));
    }
}

#[test]
fn test_tls_roots_options_debug_shows_count_without_der() {
    assert!(HttpClientOptions::default()
        .extra_root_certificates_der
        .is_empty());
    let mut options = HttpClientOptions::default();
    options
        .extra_root_certificates_der
        .push(bytes::Bytes::from_static(b"secret-der-marker"));
    let debug = format!("{options:?}");
    assert!(debug.contains("extra_root_certificates_der_count: 1"));
    assert!(!debug.contains("secret-der-marker"));
}

#[test]
#[allow(deprecated)]
fn test_tls_roots_legacy_create_factory_uses_supplied_options() {
    let options = trusted_options();
    let expected = options.extra_root_certificates_der.clone();
    let client = HttpClientBuilder::new()
        .create(options)
        .expect("legacy create builds with additional roots");
    assert_eq!(client.options().extra_root_certificates_der, expected);
}

#[tokio::test]
async fn test_tls_roots_clone_to_builder_and_rebuild_keep_roots() {
    let tunnel = timeout(TEST_TIMEOUT, TlsTunnel::start())
        .await
        .expect("startup timeout")
        .expect("start tunnel");
    let mut client_options = options(tunnel.proxy_addr());
    client_options.extra_root_certificates_der = trusted_options().extra_root_certificates_der;
    let client = HttpClient::builder()
        .options(client_options.clone())
        .build()
        .expect("client build");
    let cloned = client_options.clone();
    assert_eq!(
        cloned.extra_root_certificates_der,
        client_options.extra_root_certificates_der
    );
    let rebuilt = client
        .to_builder()
        .build()
        .expect("to_builder retains roots");
    assert_eq!(
        rebuilt.options().extra_root_certificates_der,
        client_options.extra_root_certificates_der
    );
    let error = client
        .rebuild_with_options({
            let mut invalid = HttpClientOptions::default();
            invalid
                .extra_root_certificates_der
                .push(bytes::Bytes::new());
            invalid
        })
        .expect_err("invalid replacement must fail");
    assert_eq!(error.kind(), HttpErrorKind::BuildClient);
    assert_eq!(
        client.options().extra_root_certificates_der,
        client_options.extra_root_certificates_der
    );
    assert_eq!(
        tls_client_request(&client, "localhost", tunnel.tls_port())
            .await
            .expect("original client remains trusted after failed rebuild"),
        "tls tunnel response body"
    );
    let observation = timeout(TEST_TIMEOUT, tunnel.finish())
        .await
        .expect("server finish timeout");
    assert!(observation.tls_handshake_succeeded);

    let tunnel = timeout(TEST_TIMEOUT, TlsTunnel::start())
        .await
        .expect("TLS tunnel startup timeout")
        .expect("start TLS tunnel");
    let mut second_options = options(tunnel.proxy_addr());
    second_options.extra_root_certificates_der = trusted_options().extra_root_certificates_der;
    let rebuilt = HttpClient::builder()
        .options(second_options)
        .build()
        .expect("second client build");
    let clone = rebuilt
        .to_builder()
        .build()
        .expect("to_builder retains roots");
    assert_eq!(
        tls_client_request(&clone, "localhost", tunnel.tls_port())
            .await
            .expect("to_builder client remains trusted"),
        "tls tunnel response body"
    );
    let observation = timeout(TEST_TIMEOUT, tunnel.finish())
        .await
        .expect("server finish timeout");
    assert!(observation.tls_handshake_succeeded);
}

#[tokio::test]
async fn test_tls_roots_cleared_replacement_rejects_private_ca() {
    let tunnel = timeout(TEST_TIMEOUT, TlsTunnel::start())
        .await
        .expect("startup timeout")
        .expect("start tunnel");
    let mut options = options(tunnel.proxy_addr());
    options.extra_root_certificates_der = trusted_options().extra_root_certificates_der;
    let client = HttpClient::builder()
        .options(options)
        .build()
        .expect("trusted client build");
    let mut replacement = client.options().clone();
    replacement.extra_root_certificates_der.clear();
    let replacement_client = client
        .rebuild_with_options(replacement)
        .expect("empty replacement roots are valid");
    assert!(replacement_client
        .options()
        .extra_root_certificates_der
        .is_empty());
    let error = tls_client_request(&replacement_client, "localhost", tunnel.tls_port())
        .await
        .expect_err("cleared roots must reject private CA");
    assert!(matches!(
        error.kind(),
        HttpErrorKind::Transport | HttpErrorKind::BuildClient | HttpErrorKind::Other
    ));
    let observation = timeout(TEST_TIMEOUT, tunnel.finish())
        .await
        .expect("server finish timeout");
    assert!(!observation.tls_handshake_succeeded);
    assert!(observation.application_request.is_none());
}
