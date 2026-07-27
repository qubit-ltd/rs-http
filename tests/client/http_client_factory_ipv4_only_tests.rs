// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use http::Method;
use qubit_http::{
    HttpClientFactory,
    HttpClientOptions,
    HttpErrorKind,
};
use tokio::time::timeout;

use crate::common::{
    spawn_one_shot_server,
    ResponsePlan,
};

#[test]
fn test_ipv4_only_option_is_preserved_in_client_options() {
    let mut options = HttpClientOptions::default();
    options.ipv4_only = true;
    let client = HttpClientFactory::new().create(options).unwrap();
    assert!(client.options().ipv4_only);
}

#[tokio::test]
async fn test_ipv4_only_with_localhost_request_is_accessible() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ipv4-only-ok".to_vec(),
    })
    .await;

    let mut localhost_url = server.base_url();
    localhost_url
        .set_host(Some("localhost"))
        .expect("failed to set localhost host");

    let mut options = HttpClientOptions::default();
    options.base_url = Some(localhost_url);
    options.ipv4_only = true;
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client.request(Method::GET, "/ipv4-check").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().await.unwrap(), "ipv4-only-ok");
}

#[tokio::test]
async fn test_ipv4_only_rejects_ipv6_literal_request_url() {
    let mut options = HttpClientOptions::default();
    options.ipv4_only = true;
    options.timeouts.write_timeout = Duration::from_secs(1);
    options.timeouts.read_timeout = Duration::from_secs(1);

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client
        .request(Method::GET, "http://[::1]:18080/ipv6")
        .build();
    let error = client.execute(request).await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::InvalidUrl);
    assert!(error.message.contains("ipv4_only=true"));
}

#[test]
fn test_ipv4_only_rejects_ipv6_literal_proxy_host() {
    let mut options = HttpClientOptions::default();
    options.ipv4_only = true;
    options.proxy.enabled = true;
    options.proxy.host = Some("[::1]".to_string());
    options.proxy.port = Some(8080);

    let error = HttpClientFactory::new().create(options).unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::ProxyConfig);
    assert!(error.message.contains("not allowed when ipv4_only=true"));
}

#[tokio::test]
async fn test_ipv4_only_fails_on_hostname_without_ipv4_address() {
    let mut options = HttpClientOptions::default();
    options
        .set_base_url("http://ip6-localhost")
        .expect("base URL should parse");
    options.ipv4_only = true;
    options.timeouts.write_timeout = Duration::from_secs(1);
    options.timeouts.read_timeout = Duration::from_secs(1);

    let client = HttpClientFactory::new().create(options).unwrap();
    let request = client.request(Method::GET, "/only-ipv6").build();
    let error = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap_err();

    assert!(
        matches!(
            error.kind,
            HttpErrorKind::Transport | HttpErrorKind::WriteTimeout
        ),
        "expected IPv4-only DNS failure to be transport or write timeout, got {:?}",
        error.kind
    );
}
