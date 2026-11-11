// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::process::Stdio;
use std::time::Duration;

use http::Method;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use qubit_http::HttpOriginPolicy;
use qubit_http::ProxyType;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::test as tokio_test;
use tokio::time::timeout;

use crate::common::spawn_one_shot_server;
use crate::common::spawn_simple_proxy_server;
use crate::common::ProxyBehavior;
use crate::common::ResponsePlan;

#[test]
fn test_http_proxy_rejects_path_in_host_before_building_client() {
    let mut options = HttpClientOptions::default();
    options.proxy.enabled = true;
    options.proxy.host = Some("127.0.0.1/".to_string());
    options.proxy.port = Some(1234);
    let error = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect_err("proxy host containing a path must be rejected before any request");
    assert_eq!(error.kind, HttpErrorKind::ProxyConfig);
    assert!(error.message.contains("proxy.host"));
}

#[tokio_test]
async fn test_http_proxy_forwards_request_and_sends_proxy_auth() {
    let backend = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![("Content-Type".to_string(), "application/json".to_string())],
        body: br#"{"ok":true}"#.to_vec(),
    })
    .await;
    let proxy = spawn_simple_proxy_server(ProxyBehavior::ForwardHttp).await;

    let mut options = HttpClientOptions::default();
    options.origin_policy = HttpOriginPolicy::AnyOrigin;
    options.base_url = Some(backend.base_url());
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Http;
    options.proxy.host = Some(proxy.host().to_string());
    options.proxy.port = Some(proxy.port());
    options.proxy.username = Some("user".to_string());
    options.proxy.password = Some("pass".to_string());
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);

    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client.request(Method::GET, "/via-proxy").build();
    let response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);

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

#[tokio_test]
async fn test_env_proxy_isolated_in_child_process() {
    for scenario in ["direct", "proxy"] {
        let backend = spawn_one_shot_server(ResponsePlan::Immediate {
            status: 200,
            headers: vec![],
            body: b"ok".to_vec(),
        })
        .await;
        let proxy = if scenario == "proxy" {
            Some(spawn_simple_proxy_server(ProxyBehavior::ForwardHttp).await)
        } else {
            None
        };
        let proxy_url = proxy
            .as_ref()
            .map(|proxy| format!("http://{}:{}", proxy.host(), proxy.port()))
            .unwrap_or_else(|| "http://127.0.0.1:1".to_string());
        let mut child =
            tokio::process::Command::new(std::env::current_exe().expect("test executable"))
                .arg("--exact")
                .arg("client::http_client_factory_proxy_tests::test_env_proxy_worker")
                .arg("--nocapture")
                .env_remove("HTTP_PROXY")
                .env_remove("http_proxy")
                .env_remove("HTTPS_PROXY")
                .env_remove("https_proxy")
                .env_remove("ALL_PROXY")
                .env_remove("all_proxy")
                .env_remove("NO_PROXY")
                .env_remove("no_proxy")
                .env_remove("REQUEST_METHOD")
                .env("RS_HTTP_PROXY_WORKER", scenario)
                .env("RS_HTTP_BACKEND_URL", backend.base_url().to_string())
                .env("HTTP_PROXY", &proxy_url)
                .env("HTTPS_PROXY", &proxy_url)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("worker process should start");
        let mut stdout = child.stdout.take().expect("child stdout is piped");
        let mut stderr = child.stderr.take().expect("child stderr is piped");
        let stdout_task = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stdout
                .read_to_end(&mut bytes)
                .await
                .expect("read child stdout");
            bytes
        });
        let stderr_task = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stderr
                .read_to_end(&mut bytes)
                .await
                .expect("read child stderr");
            bytes
        });
        let status = match timeout(Duration::from_secs(30), child.wait()).await {
            Ok(result) => result.expect("worker process should exit"),
            Err(_) => {
                let _ = child.kill().await;
                let _ = child
                    .wait()
                    .await
                    .expect("timed-out worker should be reaped");
                let stdout = stdout_task.await.expect("stdout drain task should finish");
                let stderr = stderr_task.await.expect("stderr drain task should finish");
                panic!(
                    "worker process timed out; stdout={} stderr={}",
                    String::from_utf8_lossy(&stdout),
                    String::from_utf8_lossy(&stderr)
                );
            }
        };
        let stdout = stdout_task.await.expect("stdout drain task should finish");
        let stderr = stderr_task.await.expect("stderr drain task should finish");
        assert!(
            status.success(),
            "worker failed: {}",
            String::from_utf8_lossy(&stderr)
        );
        assert!(
            String::from_utf8_lossy(&stdout).contains("RS_HTTP_PROXY_WORKER_OK"),
            "worker did not run: {}",
            String::from_utf8_lossy(&stdout)
        );
        if scenario == "proxy" {
            let captured = timeout(
                Duration::from_secs(3),
                proxy.expect("proxy scenario has a proxy").finish(),
            )
            .await
            .expect("proxy did not finish");
            assert!(captured.target.contains("/env-proxy"));
            let _ = timeout(Duration::from_secs(3), backend.finish())
                .await
                .expect("backend did not finish");
        } else {
            let captured = timeout(Duration::from_secs(3), backend.finish())
                .await
                .expect("direct backend did not finish");
            assert_eq!(captured.target, "/env-proxy");
        }
    }
}

#[tokio_test]
async fn test_env_proxy_ipv6_with_ipv4_only_isolated_in_child_process() {
    for scenario in ["env_ipv6", "direct_ipv4", "explicit_ipv6"] {
        let backend = if scenario == "direct_ipv4" {
            Some(
                spawn_one_shot_server(ResponsePlan::Immediate {
                    status: 200,
                    headers: vec![],
                    body: b"ok".to_vec(),
                })
                .await,
            )
        } else {
            None
        };
        let mut proxy = Some(
            TcpListener::bind("[::1]:0")
                .await
                .expect("IPv6 loopback proxy must bind for this check"),
        );
        let proxy_addr = proxy
            .as_ref()
            .expect("proxy listener")
            .local_addr()
            .expect("proxy address");
        let proxy_url = format!("http://[::1]:{}", proxy_addr.port());
        let backend_url = backend
            .as_ref()
            .map(|backend| backend.base_url())
            .unwrap_or_else(|| url::Url::parse("http://127.0.0.1:9/").expect("IPv4 target URL"));
        let mut child =
            tokio::process::Command::new(std::env::current_exe().expect("test executable"))
                .arg("--exact")
                .arg("client::http_client_factory_proxy_tests::test_env_proxy_worker")
                .arg("--nocapture")
                .env_remove("HTTP_PROXY")
                .env_remove("http_proxy")
                .env_remove("HTTPS_PROXY")
                .env_remove("https_proxy")
                .env_remove("ALL_PROXY")
                .env_remove("all_proxy")
                .env_remove("NO_PROXY")
                .env_remove("no_proxy")
                .env_remove("REQUEST_METHOD")
                .env("RS_HTTP_PROXY_WORKER", scenario)
                .env("RS_HTTP_BACKEND_URL", backend_url.to_string())
                .env("HTTP_PROXY", &proxy_url)
                .env("HTTPS_PROXY", &proxy_url)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("worker process should start");
        let proxy_task = if scenario == "env_ipv6" {
            let proxy = proxy.take().expect("proxy listener");
            Some(tokio::spawn(async move {
                let (mut stream, _) = timeout(Duration::from_secs(5), proxy.accept())
                    .await
                    .expect("environment proxy was not accepted")
                    .expect("accept proxy connection");
                let mut request = Vec::new();
                timeout(Duration::from_secs(3), async {
                    let mut chunk = [0; 1024];
                    loop {
                        let bytes_read = stream.read(&mut chunk).await.expect("read proxy request");
                        assert_ne!(bytes_read, 0, "proxy closed before sending request headers");
                        request.extend_from_slice(&chunk[..bytes_read]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                })
                .await
                .expect("read proxy request timed out");
                assert!(
                    String::from_utf8_lossy(&request).contains("http://127.0.0.1:9/env-proxy"),
                    "proxy request must target the IPv4 loopback URL"
                );
                timeout(
                    Duration::from_secs(3),
                    stream.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    ),
                )
                .await
                .expect("write proxy response timed out")
                .expect("write proxy response");
            }))
        } else {
            None
        };
        let mut stdout = child.stdout.take().expect("child stdout is piped");
        let mut stderr = child.stderr.take().expect("child stderr is piped");
        let stdout_task = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).await.expect("read stdout");
            bytes
        });
        let stderr_task = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).await.expect("read stderr");
            bytes
        });
        let status = match timeout(Duration::from_secs(30), child.wait()).await {
            Ok(result) => result.expect("worker process should exit"),
            Err(_) => {
                let _ = child.kill().await;
                let _ = child
                    .wait()
                    .await
                    .expect("timed-out worker should be reaped");
                let stdout = stdout_task.await.expect("stdout drain");
                let stderr = stderr_task.await.expect("stderr drain");
                panic!(
                    "worker timed out; stdout={} stderr={}",
                    String::from_utf8_lossy(&stdout),
                    String::from_utf8_lossy(&stderr)
                );
            }
        };
        let stdout = stdout_task.await.expect("stdout drain task");
        let stderr = stderr_task.await.expect("stderr drain task");
        assert!(
            status.success(),
            "worker failed: {}",
            String::from_utf8_lossy(&stderr)
        );
        assert!(
            String::from_utf8_lossy(&stdout).contains("RS_HTTP_PROXY_WORKER_OK"),
            "worker did not run: {}",
            String::from_utf8_lossy(&stdout)
        );
        match scenario {
            "env_ipv6" => {
                proxy_task
                    .expect("environment proxy task")
                    .await
                    .expect("proxy task should finish");
                println!("OBSERVATION env_ipv6 accepted=true");
            }
            "direct_ipv4" => {
                let captured = timeout(
                    Duration::from_secs(3),
                    backend.expect("direct scenario backend").finish(),
                )
                .await
                .expect("direct backend did not finish");
                assert_eq!(captured.target, "/env-proxy");
                assert!(
                    timeout(
                        Duration::from_millis(300),
                        proxy.as_ref().expect("proxy listener").accept()
                    )
                    .await
                    .is_err(),
                    "direct mode unexpectedly connected to the proxy"
                );
                println!("OBSERVATION direct_ipv4 target_success=true proxy_accepted=false");
            }
            _ => {
                assert!(
                    timeout(
                        Duration::from_millis(300),
                        proxy.as_ref().expect("proxy listener").accept()
                    )
                    .await
                    .is_err(),
                    "explicit proxy should be rejected before connecting"
                );
                println!("OBSERVATION explicit_ipv6 error=ProxyConfig");
            }
        }
    }
}

#[tokio_test]
async fn test_env_proxy_worker() {
    let Ok(scenario) = std::env::var("RS_HTTP_PROXY_WORKER") else {
        return;
    };
    let backend_url = std::env::var("RS_HTTP_BACKEND_URL").expect("backend URL supplied by parent");
    let mut options = HttpClientOptions::default();
    options.base_url = Some(url::Url::parse(&backend_url).expect("valid backend URL"));
    options.use_env_proxy = scenario == "proxy" || scenario == "env_ipv6";
    options.ipv4_only =
        scenario == "env_ipv6" || scenario == "direct_ipv4" || scenario == "explicit_ipv6";
    if scenario == "explicit_ipv6" {
        options.proxy.enabled = true;
        options.proxy.host = Some("::1".to_string());
        options.proxy.port = Some(9);
        let error = HttpClientBuilder::new()
            .options(options)
            .build()
            .expect_err("explicit IPv6 proxy must be rejected");
        assert_eq!(error.kind, HttpErrorKind::ProxyConfig);
        println!("RS_HTTP_PROXY_WORKER_OK");
        return;
    }
    options.timeouts.response_header_timeout = Duration::from_secs(3);
    let client = HttpClientBuilder::new()
        .options(options)
        .build()
        .expect("client should be created");
    let response = client
        .execute(client.request(Method::GET, "/env-proxy").build())
        .await
        .expect("request should succeed");
    assert_eq!(response.status().as_u16(), 200);
    println!("RS_HTTP_PROXY_WORKER_OK");
}

#[tokio_test]
async fn test_https_via_http_proxy_uses_connect_tunnel() {
    let proxy = spawn_simple_proxy_server(ProxyBehavior::ConnectProbe).await;

    let mut options = HttpClientOptions::default();
    options.origin_policy = HttpOriginPolicy::AnyOrigin;
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Http;
    options.proxy.host = Some(proxy.host().to_string());
    options.proxy.port = Some(proxy.port());
    options.proxy.username = Some("user".to_string());
    options.proxy.password = Some("pass".to_string());
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);
    options.timeouts.request_timeout = Some(Duration::from_secs(2));

    let client = HttpClientBuilder::new().options(options).build().unwrap();
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
