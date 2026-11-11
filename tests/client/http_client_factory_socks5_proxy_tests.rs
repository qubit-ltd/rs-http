// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use http::Method;
use qubit_http::HttpClientBuilder;
use qubit_http::HttpClientOptions;
use qubit_http::ProxyType;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::spawn;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::test as tokio_test;
use tokio::time::timeout;

use crate::common::ResponsePlan;
use crate::common::spawn_one_shot_server;

#[derive(Debug)]
struct SocksServer {
    host: String,
    port: u16,
    join_handle: JoinHandle<()>,
    target_rx: oneshot::Receiver<(u8, String, u16)>,
}

#[derive(Debug)]
struct AuthSocksServer {
    host: String,
    port: u16,
    join_handle: JoinHandle<()>,
    result_rx: oneshot::Receiver<AuthResult>,
}

#[derive(Debug)]
struct AuthResult {
    username: String,
    password: String,
    accepted: bool,
    target: Option<(u8, String, u16)>,
}

impl AuthSocksServer {
    async fn finish(mut self) -> AuthResult {
        let result = timeout(Duration::from_secs(5), &mut self.result_rx)
            .await
            .expect("auth result timed out")
            .expect("auth result sender dropped");
        timeout(Duration::from_secs(5), &mut self.join_handle)
            .await
            .expect("auth socks server timed out")
            .expect("auth socks server panicked");
        result
    }
}

impl Drop for AuthSocksServer {
    fn drop(&mut self) {
        self.join_handle.abort();
    }
}

async fn spawn_auth_socks5_server(expected_password: &'static str, backend_port: Option<u16>) -> AuthSocksServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind auth socks server");
    let addr = listener.local_addr().expect("failed to query auth socks address");
    let (result_tx, result_rx) = oneshot::channel();
    let join_handle = spawn(async move {
        let (mut stream, _) = timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("auth accept timed out")
            .expect("auth accept failed");
        let result = async {
            let mut greeting = [0_u8; 2];
            timeout(Duration::from_secs(5), stream.read_exact(&mut greeting))
                .await
                .expect("greeting read timed out")
                .expect("greeting read failed");
            let mut methods = vec![0; greeting[1] as usize];
            timeout(Duration::from_secs(5), stream.read_exact(&mut methods))
                .await
                .expect("methods read timed out")
                .expect("methods read failed");
            assert!(
                methods.contains(&0x02),
                "client did not offer username/password auth: {methods:?}"
            );
            timeout(Duration::from_secs(5), stream.write_all(&[0x05, 0x02]))
                .await
                .expect("method reply timed out")
                .expect("method reply failed");
            let mut auth_head = [0_u8; 2];
            timeout(Duration::from_secs(5), stream.read_exact(&mut auth_head))
                .await
                .expect("auth header timed out")
                .expect("auth header read failed");
            let mut username = vec![0; auth_head[1] as usize];
            timeout(Duration::from_secs(5), stream.read_exact(&mut username))
                .await
                .expect("username timed out")
                .expect("username read failed");
            let mut plen = [0_u8; 1];
            timeout(Duration::from_secs(5), stream.read_exact(&mut plen))
                .await
                .expect("password length timed out")
                .expect("password length read failed");
            let mut password = vec![0; plen[0] as usize];
            timeout(Duration::from_secs(5), stream.read_exact(&mut password))
                .await
                .expect("password timed out")
                .expect("password read failed");
            assert_eq!(auth_head[0], 0x01);
            let accepted = username.as_slice() == b"socks-user" && password.as_slice() == expected_password.as_bytes();
            timeout(
                Duration::from_secs(5),
                stream.write_all(&[0x01, if accepted { 0x00 } else { 0x01 }]),
            )
            .await
            .expect("auth result reply timed out")
            .expect("auth result reply failed");
            let mut result = AuthResult {
                username: String::from_utf8(username).expect("username utf8"),
                password: String::from_utf8(password).expect("password utf8"),
                accepted,
                target: None,
            };
            if !accepted {
                return result;
            }
            let target = timeout(Duration::from_secs(5), read_socks_target(&mut stream))
                .await
                .expect("CONNECT target read timed out")
                .expect("CONNECT target read failed");
            result.target = Some(target.clone());
            timeout(
                Duration::from_secs(5),
                stream.write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0]),
            )
            .await
            .expect("CONNECT reply timed out")
            .expect("CONNECT reply failed");
            if let Some(port) = backend_port {
                let mut upstream = timeout(Duration::from_secs(5), TcpStream::connect(("127.0.0.1", port)))
                    .await
                    .expect("backend connect timed out")
                    .expect("backend connect failed");
                let mut request = Vec::new();
                timeout(Duration::from_secs(5), read_http_message(&mut stream, &mut request))
                    .await
                    .expect("request read timed out")
                    .expect("request read failed");
                timeout(Duration::from_secs(5), upstream.write_all(&request))
                    .await
                    .expect("forward timed out")
                    .expect("forward failed");
                let mut response = Vec::new();
                timeout(Duration::from_secs(5), upstream.read_to_end(&mut response))
                    .await
                    .expect("backend response timed out")
                    .expect("backend response read failed");
                timeout(Duration::from_secs(5), stream.write_all(&response))
                    .await
                    .expect("response relay timed out")
                    .expect("response relay failed");
            } else {
                let mut request = Vec::new();
                timeout(Duration::from_secs(5), read_http_message(&mut stream, &mut request))
                    .await
                    .expect("request read timed out")
                    .expect("request read failed");
                assert!(
                    !String::from_utf8_lossy(&request)
                        .to_ascii_lowercase()
                        .contains("proxy-authorization:")
                );
                timeout(
                    Duration::from_secs(5),
                    stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n"),
                )
                .await
                .expect("response write timed out")
                .expect("response write failed");
            }
            result
        }
        .await;
        let _ = result_tx.send(result);
    });
    AuthSocksServer {
        host: addr.ip().to_string(),
        port: addr.port(),
        join_handle,
        result_rx,
    }
}

async fn read_socks_target(stream: &mut TcpStream) -> std::io::Result<(u8, String, u16)> {
    let mut head = [0_u8; 4];
    stream.read_exact(&mut head).await?;
    if head[0] != 5 || head[1] != 1 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid CONNECT"));
    }
    let host = match head[3] {
        1 => {
            let mut ip = [0; 4];
            stream.read_exact(&mut ip).await?;
            format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
        }
        3 => {
            let mut len = [0];
            stream.read_exact(&mut len).await?;
            let mut name = vec![0; len[0] as usize];
            stream.read_exact(&mut name).await?;
            String::from_utf8(name).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
        }
        atyp => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported atyp {atyp}"),
            ));
        }
    };
    let mut port = [0; 2];
    stream.read_exact(&mut port).await?;
    Ok((head[3], host, u16::from_be_bytes(port)))
}

impl SocksServer {
    fn host(&self) -> &str {
        &self.host
    }

    fn port(&self) -> u16 {
        self.port
    }

    async fn finish(self) -> (u8, String, u16) {
        let target = self.target_rx.await.expect("failed to receive socks target");
        self.join_handle.await.expect("socks server task panicked");
        target
    }
}

async fn spawn_socks5_server() -> SocksServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind socks5 server");
    let addr = listener.local_addr().expect("failed to query socks5 address");
    let (target_tx, target_rx) = oneshot::channel::<(u8, String, u16)>();

    let join_handle = spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("failed to accept socks5");
        let (atyp, host, port) = socks5_handshake_and_target(&mut stream)
            .await
            .expect("failed socks5 handshake");
        let _ = target_tx.send((atyp, host.clone(), port));

        let mut upstream = TcpStream::connect((host.as_str(), port))
            .await
            .expect("failed to connect upstream from socks5");

        // One-request relay is enough for this test.
        let mut request = Vec::new();
        read_http_message(&mut stream, &mut request)
            .await
            .expect("failed to read proxied request");
        upstream
            .write_all(&request)
            .await
            .expect("failed to forward proxied request");
        upstream.flush().await.expect("failed to flush upstream");

        let mut response = Vec::new();
        upstream
            .read_to_end(&mut response)
            .await
            .expect("failed to read proxied response");
        stream
            .write_all(&response)
            .await
            .expect("failed to write proxied response");
        stream.flush().await.expect("failed to flush proxied response");
    });

    SocksServer {
        host: addr.ip().to_string(),
        port: addr.port(),
        join_handle,
        target_rx,
    }
}

async fn socks5_handshake_and_target(stream: &mut TcpStream) -> std::io::Result<(u8, String, u16)> {
    let mut greeting = [0_u8; 2];
    stream.read_exact(&mut greeting).await?;
    let nmethods = greeting[1] as usize;
    let mut methods = vec![0_u8; nmethods];
    stream.read_exact(&mut methods).await?;
    stream.write_all(&[0x05, 0x00]).await?; // no-auth

    let mut head = [0_u8; 4];
    stream.read_exact(&mut head).await?;
    if head[0] != 0x05 || head[1] != 0x01 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid SOCKS5 CONNECT command",
        ));
    }
    let atyp = head[3];
    let host = match atyp {
        0x01 => {
            let mut ip = [0_u8; 4];
            stream.read_exact(&mut ip).await?;
            format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
        }
        0x03 => {
            let mut len = [0_u8; 1];
            stream.read_exact(&mut len).await?;
            let mut domain = vec![0_u8; len[0] as usize];
            stream.read_exact(&mut domain).await?;
            String::from_utf8(domain)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("bad domain: {error}")))?
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported atyp: {atyp}"),
            ));
        }
    };

    let mut port_bytes = [0_u8; 2];
    stream.read_exact(&mut port_bytes).await?;
    let port = u16::from_be_bytes(port_bytes);

    // Success with 127.0.0.1:0 as bound addr.
    let reply = [0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0];
    stream.write_all(&reply).await?;
    Ok((atyp, host, port))
}

async fn spawn_socks5_capture_server() -> SocksServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind socks5 server");
    let addr = listener.local_addr().expect("failed to query socks5 address");
    let (target_tx, target_rx) = oneshot::channel::<(u8, String, u16)>();
    let join_handle = spawn(async move {
        let (mut stream, _) = timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("socks accept timed out")
            .expect("failed to accept socks5");
        let target = timeout(Duration::from_secs(5), socks5_handshake_and_target(&mut stream))
            .await
            .expect("socks handshake timed out")
            .expect("failed socks5 handshake");
        let _ = target_tx.send(target);
        let mut request = Vec::new();
        timeout(Duration::from_secs(5), read_http_message(&mut stream, &mut request))
            .await
            .expect("proxied request read timed out")
            .expect("failed to read proxied request");
        timeout(
            Duration::from_secs(5),
            stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n"),
        )
        .await
        .expect("capture response write timed out")
        .expect("failed to write capture response");
    });
    SocksServer {
        host: addr.ip().to_string(),
        port: addr.port(),
        join_handle,
        target_rx,
    }
}

async fn read_http_message(stream: &mut TcpStream, output: &mut Vec<u8>) -> std::io::Result<()> {
    let header_end = loop {
        let mut buf = [0_u8; 1024];
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "closed before http headers",
            ));
        }
        output.extend_from_slice(&buf[..n]);
        if let Some(idx) = find_subsequence(output, b"\r\n\r\n") {
            break idx + 4;
        }
    };

    let header_text = String::from_utf8_lossy(&output[..header_end]);
    let mut content_length = 0usize;
    for line in header_text.split("\r\n") {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }

    let mut have = output.len() - header_end;
    while have < content_length {
        let mut chunk = vec![0_u8; content_length - have];
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        output.extend_from_slice(&chunk[..n]);
        have += n;
    }
    Ok(())
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[tokio_test]
async fn test_socks5_proxy_forwards_http_request() {
    let backend = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"ok-through-socks".to_vec(),
    })
    .await;
    let socks = spawn_socks5_server().await;

    let mut options = HttpClientOptions::default();
    options.base_url = Some(backend.base_url());
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Socks5;
    options.proxy.host = Some(socks.host().to_string());
    options.proxy.port = Some(socks.port());
    options.timeouts.response_header_timeout = Duration::from_secs(3);
    options.timeouts.read_timeout = Duration::from_secs(3);
    options.timeouts.request_timeout = Some(Duration::from_secs(3));

    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let request = client.request(Method::GET, "/socks").build();
    let mut response = timeout(Duration::from_secs(5), client.execute(request))
        .await
        .expect("execute timed out")
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().await.unwrap(), "ok-through-socks");

    let target = timeout(Duration::from_secs(5), socks.finish())
        .await
        .expect("socks finish timed out");
    assert_eq!(target.2, backend.base_url().port().unwrap());

    let captured = timeout(Duration::from_secs(3), backend.finish())
        .await
        .expect("backend finish timed out");
    assert_eq!(captured.target, "/socks");
}

#[tokio_test]
async fn test_socks5_ipv4_only_delegates_target_dns() {
    let socks = spawn_socks5_capture_server().await;
    let mut options = HttpClientOptions::default();
    options.set_base_url("http://ipv4-contract.invalid:18080").unwrap();
    options.ipv4_only = true;
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Socks5;
    options.proxy.host = Some(socks.host().to_string());
    options.proxy.port = Some(socks.port());
    options.timeouts.request_timeout = Some(Duration::from_secs(5));
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let response = timeout(
        Duration::from_secs(5),
        client.execute(client.request(Method::GET, "/dns-boundary").build()),
    )
    .await
    .expect("execute timed out")
    .expect("capture response should succeed");
    assert_eq!(response.status().as_u16(), 204);
    let target = timeout(Duration::from_secs(5), socks.finish())
        .await
        .expect("socks finish timed out");
    assert_eq!(target, (0x03, "ipv4-contract.invalid".to_owned(), 18080));
}

fn auth_proxy_options(
    proxy_host: String,
    proxy_port: u16,
    base_url: &str,
    password: Option<&str>,
) -> HttpClientOptions {
    let mut options = HttpClientOptions::default();
    options.set_base_url(base_url).unwrap();
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Socks5;
    options.proxy.host = Some(proxy_host);
    options.proxy.port = Some(proxy_port);
    options.proxy.username = Some("socks-user".to_owned());
    options.proxy.password = password.map(str::to_owned);
    options.timeouts.request_timeout = Some(Duration::from_secs(5));
    options
}

#[tokio_test]
async fn test_socks5_auth_success() {
    let backend = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"authenticated".to_vec(),
    })
    .await;
    let backend_url = backend.base_url();
    let backend_port = backend_url.port().expect("backend has a port");
    let socks = spawn_auth_socks5_server("socks-pass", Some(backend_port)).await;
    let options = auth_proxy_options(socks.host.clone(), socks.port, backend_url.as_str(), Some("socks-pass"));
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let mut response = timeout(
        Duration::from_secs(5),
        client.execute(client.request(Method::GET, "/auth").build()),
    )
    .await
    .expect("execute timed out")
    .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        timeout(Duration::from_secs(5), response.text())
            .await
            .expect("body timed out")
            .unwrap(),
        "authenticated"
    );
    let result = socks.finish().await;
    assert_eq!(result.username, "socks-user");
    assert_eq!(result.password, "socks-pass");
    assert!(result.accepted);
    assert_eq!(result.target.as_ref().unwrap().2, backend_port);
    let captured = timeout(Duration::from_secs(5), backend.finish())
        .await
        .expect("backend finish timed out");
    assert_eq!(captured.target, "/auth");
    assert!(!captured.headers.contains_key("proxy-authorization"));
}

#[tokio_test]
async fn test_socks5_auth_rejects_bad_password() {
    let socks = spawn_auth_socks5_server("expected", None).await;
    let options = auth_proxy_options(
        socks.host.clone(),
        socks.port,
        "http://upstream.invalid:18080",
        Some("wrong"),
    );
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let error = timeout(
        Duration::from_secs(5),
        client.execute(client.request(Method::GET, "/reject").build()),
    )
    .await
    .expect("execute timed out")
    .unwrap_err();
    assert_eq!(error.kind, qubit_http::HttpErrorKind::Transport);
    let result = socks.finish().await;
    assert!(!result.accepted, "server did not record auth rejection");
    assert_eq!(result.password, "wrong");
    assert!(result.target.is_none(), "rejected authentication must not send CONNECT");
}

#[tokio_test]
async fn test_socks5_auth_empty_password() {
    let backend = spawn_one_shot_server(ResponsePlan::Immediate {
        status: 200,
        headers: vec![],
        body: b"empty-password".to_vec(),
    })
    .await;
    let backend_url = backend.base_url();
    let backend_port = backend_url.port().expect("backend has a port");
    let socks = spawn_auth_socks5_server("", Some(backend_port)).await;
    let options = auth_proxy_options(socks.host.clone(), socks.port, backend_url.as_str(), None);
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let mut response = timeout(
        Duration::from_secs(5),
        client.execute(client.request(Method::GET, "/empty-password").build()),
    )
    .await
    .expect("execute timed out")
    .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        timeout(Duration::from_secs(5), response.text())
            .await
            .expect("body timed out")
            .unwrap(),
        "empty-password"
    );
    let result = socks.finish().await;
    assert!(result.accepted);
    assert_eq!(result.username, "socks-user");
    assert_eq!(result.password, "");
    assert_eq!(result.target.as_ref().unwrap().2, backend_port);
    let captured = timeout(Duration::from_secs(5), backend.finish())
        .await
        .expect("backend finish timed out");
    assert!(!captured.headers.contains_key("proxy-authorization"));
}

#[tokio_test]
async fn test_socks5_auth_preserves_remote_dns() {
    let socks = spawn_auth_socks5_server("socks-pass", None).await;
    let options = auth_proxy_options(
        socks.host.clone(),
        socks.port,
        "http://remote-dns-contract.invalid:18080",
        Some("socks-pass"),
    );
    let client = HttpClientBuilder::new().options(options).build().unwrap();
    let response = timeout(
        Duration::from_secs(5),
        client.execute(client.request(Method::GET, "/remote-dns").build()),
    )
    .await
    .expect("execute timed out")
    .unwrap();
    assert_eq!(response.status().as_u16(), 204);
    let result = socks.finish().await;
    assert!(result.accepted);
    assert_eq!(
        result.target,
        Some((0x03, "remote-dns-contract.invalid".to_owned(), 18080))
    );
}
