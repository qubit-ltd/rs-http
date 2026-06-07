// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::time::Duration;

use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::{
    TcpListener,
    TcpStream,
};
use tokio::sync::oneshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyCapturedRequest {
    pub method: String,
    pub target: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyBehavior {
    CaptureOnly,
    ForwardHttp,
    ConnectProbe,
}

#[derive(Debug)]
pub struct SimpleProxyServer {
    host: String,
    port: u16,
    request_rx: oneshot::Receiver<ProxyCapturedRequest>,
    join_handle: tokio::task::JoinHandle<()>,
}

impl SimpleProxyServer {
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn finish(self) -> ProxyCapturedRequest {
        let request = self
            .request_rx
            .await
            .expect("proxy server dropped request sender");
        self.join_handle.await.expect("proxy server task panicked");
        request
    }
}

pub async fn spawn_simple_proxy_server(
    behavior: ProxyBehavior,
) -> SimpleProxyServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind simple proxy server");
    let addr = listener
        .local_addr()
        .expect("failed to query simple proxy address");
    let host = addr.ip().to_string();
    let port = addr.port();
    let (request_tx, request_rx) = oneshot::channel::<ProxyCapturedRequest>();

    let join_handle = tokio::spawn(async move {
        let (mut client_stream, _) = listener
            .accept()
            .await
            .expect("proxy server failed to accept connection");
        let request = read_http_request(&mut client_stream)
            .await
            .expect("proxy server failed to read request");
        let _ = request_tx.send(request.clone());

        match behavior {
            ProxyBehavior::CaptureOnly => {
                write_simple_response(
                    &mut client_stream,
                    502,
                    b"proxy capture only",
                )
                .await
                .expect("proxy server failed to write capture response");
            }
            ProxyBehavior::ConnectProbe => {
                if request.method.eq_ignore_ascii_case("CONNECT") {
                    write_simple_response(&mut client_stream, 200, b"")
                        .await
                        .expect(
                            "proxy server failed to write connect response",
                        );
                    tokio::time::sleep(Duration::from_millis(50)).await;
                } else {
                    write_simple_response(
                        &mut client_stream,
                        400,
                        b"expected CONNECT",
                    )
                    .await
                    .expect(
                        "proxy server failed to write connect-probe response",
                    );
                }
            }
            ProxyBehavior::ForwardHttp => {
                if request.method.eq_ignore_ascii_case("CONNECT") {
                    write_simple_response(
                        &mut client_stream,
                        501,
                        b"CONNECT not supported",
                    )
                    .await
                    .expect("proxy server failed to reject CONNECT");
                } else {
                    forward_http_request(&request, &mut client_stream)
                        .await
                        .expect("proxy server failed to forward HTTP request");
                }
            }
        }
    });

    SimpleProxyServer {
        host,
        port,
        request_rx,
        join_handle,
    }
}

async fn read_http_request(
    stream: &mut TcpStream,
) -> std::io::Result<ProxyCapturedRequest> {
    let mut buffer = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 1024];
        let read_size = tokio::time::timeout(
            Duration::from_secs(3),
            stream.read(&mut chunk),
        )
        .await
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timed out while reading proxy request headers",
            )
        })??;
        if read_size == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed before proxy request headers were complete",
            ));
        }
        buffer.extend_from_slice(&chunk[..read_size]);
        if let Some(index) = find_subsequence(&buffer, b"\r\n\r\n") {
            break index + 4;
        }
    };

    let header_bytes = &buffer[..header_end];
    let mut body = buffer[header_end..].to_vec();
    let header_text = String::from_utf8_lossy(header_bytes);
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_string();
    let target = request_parts.next().unwrap_or_default().to_string();

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(
                name.trim().to_ascii_lowercase(),
                value.trim().to_string(),
            );
        }
    }

    let content_length = headers
        .get("content-length")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    while body.len() < content_length {
        let mut chunk = vec![0_u8; content_length - body.len()];
        let read_size = stream.read(&mut chunk).await?;
        if read_size == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read_size]);
    }

    Ok(ProxyCapturedRequest {
        method,
        target,
        headers,
        body,
    })
}

async fn forward_http_request(
    request: &ProxyCapturedRequest,
    client_stream: &mut TcpStream,
) -> std::io::Result<()> {
    let (host, port, path_and_query) = parse_target(&request.target)?;
    let mut upstream = TcpStream::connect((host.as_str(), port)).await?;

    let mut outgoing =
        format!("{} {} HTTP/1.1\r\n", request.method, path_and_query);
    for (name, value) in &request.headers {
        if name.eq_ignore_ascii_case("proxy-authorization")
            || name.eq_ignore_ascii_case("proxy-connection")
        {
            continue;
        }
        outgoing.push_str(name);
        outgoing.push_str(": ");
        outgoing.push_str(value);
        outgoing.push_str("\r\n");
    }
    outgoing.push_str("\r\n");

    upstream.write_all(outgoing.as_bytes()).await?;
    if !request.body.is_empty() {
        upstream.write_all(&request.body).await?;
    }
    upstream.flush().await?;

    let mut response = Vec::new();
    upstream.read_to_end(&mut response).await?;
    client_stream.write_all(&response).await?;
    client_stream.flush().await?;
    Ok(())
}

fn parse_target(target: &str) -> std::io::Result<(String, u16, String)> {
    let url = url::Url::parse(target).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid proxy target URL '{target}': {e}"),
        )
    })?;
    let host = url.host_str().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("target URL '{target}' has no host"),
        )
    })?;
    let port = url.port_or_known_default().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("target URL '{target}' has no known port"),
        )
    })?;
    let mut path = url.path().to_string();
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    if path.is_empty() {
        path.push('/');
    }
    Ok((host.to_string(), port, path))
}

async fn write_simple_response(
    stream: &mut TcpStream,
    status: u16,
    body: &[u8],
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        _ => "Unknown",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    if !body.is_empty() {
        stream.write_all(body).await?;
    }
    stream.flush().await?;
    Ok(())
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
