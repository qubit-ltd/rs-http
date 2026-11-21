// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error as StdError;
use std::net::TcpListener;
use std::time::Duration;

use http::Method;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::HttpErrorKind;
use qubit_http::HttpOriginPolicy;
use qubit_redact::RedactionPolicy;
use qubit_redact::Sensitivity;
use qubit_redact::formats::http::UrlPathPolicy;
use tokio::io::AsyncWriteExt;
use tokio::test as tokio_test;
use tokio::time::timeout;

use crate::common::ResponsePlan;
use crate::common::read_request;
use crate::common::spawn_one_shot_server;

#[test]
fn test_ipv4_only_option_is_preserved_in_client_options() {
    let mut options = HttpClientOptions::default();
    options.ipv4_only = true;
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    assert!(client.options().ipv4_only);
}

#[tokio_test]
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
    options.timeouts.response_header_timeout = Duration::from_secs(2);
    options.timeouts.read_timeout = Duration::from_secs(2);

    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client.request(Method::GET, "/ipv4-check").build();
    let mut response = timeout(Duration::from_secs(3), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().await.unwrap(), "ipv4-only-ok");
}

#[tokio_test]
async fn test_ipv4_only_rejects_ipv6_literal_request_url() {
    let mut options = HttpClientOptions::default();
    options.ipv4_only = true;
    options.log_redaction_policy = RedactionPolicy::strict()
        .to_builder()
        .http(|http| {
            http.url_path(UrlPathPolicy::Redact);
            let _ = http.query().raise("access_token", Sensitivity::Secret);
        })
        .unwrap()
        .build()
        .unwrap();
    options.timeouts.response_header_timeout = Duration::from_secs(1);
    options.timeouts.read_timeout = Duration::from_secs(1);

    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client
        .request(
            Method::GET,
            "http://[::1]:18080/PRIVATE_IPV6_PATH?access_token=PRIVATE_IPV6_QUERY",
        )
        .build();
    let error = client.execute(request).await.unwrap_err();

    assert_eq!(error.kind, HttpErrorKind::InvalidUrl);
    assert_eq!(error.message(), "IPv6 literal host is not allowed when ipv4_only=true");
    let url = error.url().expect("rejected candidate URL should be retained");
    assert_eq!(url.path(), "/PRIVATE_IPV6_PATH");
    assert_eq!(url.query(), Some("access_token=PRIVATE_IPV6_QUERY"));
    for output in [error.to_string(), format!("{error:?}")] {
        assert!(!output.contains("PRIVATE_IPV6_PATH"));
        assert!(!output.contains("PRIVATE_IPV6_QUERY"));
    }
}

#[tokio_test]
async fn test_ipv4_only_initial_literal_policy_comes_from_request_snapshot() {
    let listener = match TcpListener::bind("[::1]:0") {
        Ok(listener) => listener,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::Unsupported
            ) =>
        {
            eprintln!("IPv6 loopback is unavailable: {error}");
            return;
        }
        Err(error) => panic!("bind IPv6 loopback listener: {error}"),
    };
    listener.set_nonblocking(true).expect("set nonblocking listener");
    let url = format!("http://{}/snapshot", listener.local_addr().expect("IPv6 bind address"));

    let options = HttpClientOptions {
        use_env_proxy: false,
        ..HttpClientOptions::default()
    };
    let permissive = HttpClientBuilder::new()
        .options(options.clone())
        .build()
        .expect("build permissive client");
    let strict = HttpClientBuilder::new()
        .options(HttpClientOptions {
            ipv4_only: true,
            ..options
        })
        .build()
        .expect("build strict client");

    let deadline = Duration::from_secs(3);
    let error = timeout(deadline, strict.execute(strict.request(Method::GET, &url).build()))
        .await
        .expect("strict request must finish")
        .expect_err("strict request snapshot must reject the literal");
    assert_eq!(error.kind(), HttpErrorKind::InvalidUrl);
    assert_eq!(
        listener
            .accept()
            .expect_err("strict snapshot must reject before connecting")
            .kind(),
        std::io::ErrorKind::WouldBlock
    );

    let listener = tokio::net::TcpListener::from_std(listener).expect("register IPv6 listener");
    let mut explicitly_relaxed = strict.request(Method::GET, &url).build();
    explicitly_relaxed.set_ipv4_only(false);
    for request in [permissive.request(Method::GET, &url).build(), explicitly_relaxed] {
        let serve = async {
            let (mut socket, _) = listener.accept().await.expect("accept IPv6 connection");
            let received = read_request(&mut socket).await.expect("read IPv6 request");
            assert_eq!(received.target, "/snapshot");
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .expect("write IPv6 response");
        };
        let (result, ()) = timeout(deadline, async { tokio::join!(strict.execute(request), serve) })
            .await
            .expect("permitted request must reach the IPv6 listener");
        assert_eq!(result.expect("execute permitted IPv6 request").status().as_u16(), 200);
    }
}

#[test]
fn test_ipv4_only_rejects_ipv6_literal_proxy_host() {
    for host in ["[::1]", "::1"] {
        let mut options = HttpClientOptions::default();
        options.ipv4_only = true;
        options.proxy.enabled = true;
        options.proxy.host = Some(host.to_string());
        options.proxy.port = Some(8080);

        let error = HttpClientBuilder::new().options(options).build().unwrap_err();

        assert_eq!(error.kind, HttpErrorKind::ProxyConfig);
        assert!(error.message.contains("not allowed when ipv4_only=true"));
    }
}

#[tokio_test]
async fn test_ipv4_only_rejects_ipv6_redirect_target_as_invalid_url() {
    let server = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 302,
        headers: vec![("Location".to_string(), "http://[::1]:9/redirect-target".to_string())],
        body: vec![],
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.origin_policy = HttpOriginPolicy::AnyOrigin;
    options.ipv4_only = true;
    options.retry.enabled = false;
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let error = client
        .execute(client.request(Method::GET, "/redirect").build())
        .await
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::InvalidUrl);
    let mut source: Option<&(dyn StdError + 'static)> = error
        .source
        .as_deref()
        .map(|source| source as &(dyn StdError + 'static));
    let mut marker_found = false;
    while let Some(candidate) = source {
        if candidate.to_string() == "redirect target is IPv6 while ipv4_only=true" {
            marker_found = true;
            break;
        }
        source = candidate.source();
    }
    assert!(marker_found, "redirect marker must remain in source chain");
    let _ = server.finish().await;
}

#[tokio_test]
async fn test_ipv4_only_rejects_ipv4_mapped_ipv6_redirect() {
    let source = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 302,
        headers: vec![("Location".to_string(), "http://[::ffff:127.0.0.1]:9/mapped".to_string())],
        body: vec![],
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.origin_policy = HttpOriginPolicy::AnyOrigin;
    options.ipv4_only = true;
    options.retry.enabled = false;
    options.base_url = Some(source.base_url());
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let error = client
        .execute(client.request(Method::GET, "/redirect-mapped").build())
        .await
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::InvalidUrl);
    let mut source_error: Option<&(dyn StdError + 'static)> = error
        .source
        .as_deref()
        .map(|source| source as &(dyn StdError + 'static));
    let mut marker_found = false;
    while let Some(candidate) = source_error {
        if candidate.to_string() == "redirect target is IPv6 while ipv4_only=true" {
            marker_found = true;
            break;
        }
        source_error = candidate.source();
    }
    assert!(
        marker_found,
        "mapped IPv6 target should retain the private marker source"
    );
    let _ = source.finish().await;
}

#[tokio_test]
async fn test_ipv4_only_redirect_never_connects_to_ipv6_target() {
    let target = TcpListener::bind("[::1]:0").ok();
    if let Some(target) = target.as_ref() {
        target.set_nonblocking(true).unwrap();
    }
    let location = target
        .as_ref()
        .map(|listener| format!("http://[::1]:{}/target", listener.local_addr().unwrap().port()))
        .unwrap_or_else(|| "http://[::1]:9/target".to_string());
    let source = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 302,
        headers: vec![("Location".to_string(), location)],
        body: vec![],
    })
    .await;
    let mut options = HttpClientOptions::default();
    options.origin_policy = HttpOriginPolicy::AnyOrigin;
    options.ipv4_only = true;
    options.retry.enabled = false;
    options.base_url = Some(source.base_url());
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let error = client
        .execute(client.request(Method::GET, "/redirect-ipv6").build())
        .await
        .unwrap_err();
    assert_eq!(error.kind, HttpErrorKind::InvalidUrl);
    if let Some(target) = target {
        let error = target
            .accept()
            .expect_err("IPv4-only redirect must reject before connecting");
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    }
    let _ = source.finish().await;
}

#[tokio_test]
async fn test_ipv4_only_disabled_allows_ipv6_redirect_policy() {
    let target = tokio::net::TcpListener::bind("[::1]:0").await.ok();
    let location = target
        .as_ref()
        .map(|listener| format!("http://[::1]:{}/target", listener.local_addr().unwrap().port()))
        .unwrap_or_else(|| "http://[::1]:9/target".to_string());
    let source = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 302,
        headers: vec![("Location".to_string(), location)],
        body: vec![],
    })
    .await;
    let mut target_task = target.map(|listener| {
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
        })
    });
    let mut options = HttpClientOptions::default();
    options.origin_policy = HttpOriginPolicy::AnyOrigin;
    options.ipv4_only = false;
    options.retry.enabled = false;
    options.use_env_proxy = false;
    options.base_url = Some(source.base_url());
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let result = timeout(
        Duration::from_secs(3),
        client.execute(client.request(Method::GET, "/redirect-ipv6-disabled").build()),
    )
    .await
    .expect("redirect should finish");
    match result {
        Ok(response) => assert_eq!(response.status().as_u16(), 200),
        Err(error) => assert_ne!(
            error.kind,
            HttpErrorKind::InvalidUrl,
            "IPv6 redirect must not be rejected by the IPv4-only marker when the option is disabled"
        ),
    }
    if let Some(task) = target_task.as_mut() {
        timeout(Duration::from_secs(3), task).await.unwrap().unwrap();
    }
    let _ = source.finish().await;
}

#[tokio_test]
async fn test_ipv4_only_allows_ipv4_to_ipv4_redirect() {
    let server = crate::common::spawn_multi_shot_server(vec![
        ResponsePlan::Immediate {
            status: 302,
            headers: vec![("Location".to_string(), "/final".to_string())],
            body: vec![],
        },
        ResponsePlan::Immediate {
            status: 200,
            headers: vec![],
            body: b"ok".to_vec(),
        },
    ])
    .await;
    let mut options = HttpClientOptions::default();
    options.origin_policy = HttpOriginPolicy::AnyOrigin;
    options.ipv4_only = true;
    options.base_url = Some(server.base_url());
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let response = client
        .execute(client.request(Method::GET, "/start").build())
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(server.finish().await.len(), 2);
}
