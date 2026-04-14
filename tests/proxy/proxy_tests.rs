/*******************************************************************************
 *
 *    Copyright (c) 2025 - 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/

use std::time::Duration;

use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions, HttpErrorKind, ProxyType};
use tokio::time::timeout;

use crate::common::{
    spawn_one_shot_server, spawn_simple_proxy_server, ProxyBehavior, ResponsePlan,
};

#[tokio::test]
async fn test_http_proxy_forwards_request_and_sends_proxy_auth() {
    let backend = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![("Content-Type".to_string(), "application/json".to_string())],
        body: br#"{"ok":true}"#.to_vec(),
    })
    .await;
    let proxy = spawn_simple_proxy_server(ProxyBehavior::ForwardHttp).await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(backend.base_url());
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Http;
    options.proxy.host = Some(proxy.host().to_string());
    options.proxy.port = Some(proxy.port());
    options.proxy.username = Some("user".to_string());
    options.proxy.password = Some("pass".to_string());
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client.request(Method::GET, "/via-proxy").build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.status.as_u16(), 200);

    let proxy_captured = timeout(Duration::from_secs(3), proxy.finish())
        .await
        .expect("proxy finish timed out");
    assert_eq!(proxy_captured.method, "GET");
    assert!(proxy_captured.target.starts_with(&format!(
        "http://127.0.0.1:{}/via-proxy",
        backend.base_url().port().unwrap()
    )));
    assert_eq!(
        proxy_captured.headers.get("proxy-authorization"),
        Some(&"Basic dXNlcjpwYXNz".to_string())
    );

    let backend_captured = timeout(Duration::from_secs(3), backend.finish())
        .await
        .expect("backend finish timed out");
    assert_eq!(backend_captured.target, "/via-proxy");
    assert!(!backend_captured.headers.contains_key("proxy-authorization"));
}

#[tokio::test]
async fn test_proxy_disabled_does_not_use_environment_proxy() {
    let backend = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    })
    .await;
    let proxy = spawn_simple_proxy_server(ProxyBehavior::CaptureOnly).await;

    let proxy_url = format!("http://{}:{}", proxy.host(), proxy.port());
    std::env::set_var("HTTP_PROXY", &proxy_url);
    std::env::set_var("HTTPS_PROXY", &proxy_url);

    let mut options = HttpClientOptions::default();
    options.base_url = Some(backend.base_url());
    options.proxy.enabled = false;
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let result = client
        .execute(client.request(Method::GET, "/direct").build())
        .await;

    std::env::remove_var("HTTP_PROXY");
    std::env::remove_var("HTTPS_PROXY");

    let response = result.unwrap();
    assert_eq!(response.status.as_u16(), 200);
    let backend_captured = timeout(Duration::from_secs(3), backend.finish())
        .await
        .expect("backend finish timed out");
    assert_eq!(backend_captured.target, "/direct");
}

#[tokio::test]
async fn test_https_via_http_proxy_uses_connect_tunnel() {
    let proxy = spawn_simple_proxy_server(ProxyBehavior::ConnectProbe).await;

    let mut options = HttpClientOptions::default();
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Http;
    options.proxy.host = Some(proxy.host().to_string());
    options.proxy.port = Some(proxy.port());
    options.proxy.username = Some("user".to_string());
    options.proxy.password = Some("pass".to_string());
    options.timeouts.write_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(2));

    let client = HttpClientFactory::new()
        .create_with_options(options)
        .unwrap();
    let request = client
        .request(Method::GET, "https://example.com/through-proxy")
        .build();
    let error = client.execute(request).await.unwrap_err();
    assert!(
        matches!(
            error.kind,
            HttpErrorKind::Transport
                | HttpErrorKind::ConnectTimeout
                | HttpErrorKind::RequestTimeout
                | HttpErrorKind::Decode
                | HttpErrorKind::InvalidUrl
        ),
        "unexpected error kind for connect probe: {:?}",
        error.kind
    );

    let captured = timeout(Duration::from_secs(3), proxy.finish())
        .await
        .expect("proxy finish timed out");
    assert_eq!(captured.method, "CONNECT");
    assert!(captured.target.contains("example.com:443"));
    assert_eq!(
        captured.headers.get("proxy-authorization"),
        Some(&"Basic dXNlcjpwYXNz".to_string())
    );
}
