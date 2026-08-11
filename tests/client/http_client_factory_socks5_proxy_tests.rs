// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use http::Method;
use qubit_http::HttpClientFactory;
use qubit_http::HttpClientOptions;
use qubit_http::ProxyType;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::time::timeout;

use crate::common::spawn_one_shot_server;
use crate::common::ResponsePlan;

#[derive(Debug)]
struct SocksServer {
    host: String,
    port: u16,
    join_handle: tokio::task::JoinHandle<()>,
    target_rx: oneshot::Receiver<(String, u16)>,
}

impl SocksServer {
    fn host(&self) -> &str {
        &self.host
    }

    fn port(&self) -> u16 {
        self.port
    }

    async fn finish(self) -> (String, u16) {
        let target = self
            .target_rx
            .await
            .expect("failed to receive socks target");
        self.join_handle.await.expect("socks server task panicked");
        target
    }
}

async fn spawn_socks5_server() -> SocksServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind socks5 server");
    let addr = listener
        .local_addr()
        .expect("failed to query socks5 address");
    let (target_tx, target_rx) = oneshot::channel::<(String, u16)>();

    let join_handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("failed to accept socks5");
        let (host, port) = socks5_handshake_and_target(&mut stream)
            .await
            .expect("failed socks5 handshake");
        let _ = target_tx.send((host.clone(), port));

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
        stream
            .flush()
            .await
            .expect("failed to flush proxied response");
    });

    SocksServer {
        host: addr.ip().to_string(),
        port: addr.port(),
        join_handle,
        target_rx,
    }
}

async fn socks5_handshake_and_target(stream: &mut TcpStream) -> std::io::Result<(String, u16)> {
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
            String::from_utf8(domain).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("bad domain: {e}"))
            })?
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
    Ok((host, port))
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
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[tokio::test]
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
    options.timeouts.write_timeout = Duration::from_secs(3);
    options.timeouts.read_timeout = Duration::from_secs(3);
    options.timeouts.request_timeout = Some(Duration::from_secs(3));

    let client = HttpClientFactory::new().create(options).unwrap();
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
    assert_eq!(target.1, backend.base_url().port().unwrap());

    let captured = timeout(Duration::from_secs(3), backend.finish())
        .await
        .expect("backend finish timed out");
    assert_eq!(captured.target, "/socks");
}
