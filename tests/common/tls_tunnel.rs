// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls;
use tokio_rustls::rustls::pki_types::CertificateDer;
use tokio_rustls::rustls::pki_types::PrivateKeyDer;
use tokio_rustls::rustls::pki_types::PrivatePkcs8KeyDer;

const IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) struct TunnelObservation {
    pub(crate) connect_authority: String,
    pub(crate) proxy_authorization: Option<String>,
    pub(crate) tls_handshake_succeeded: bool,
    pub(crate) application_request: Option<String>,
}

pub(crate) struct TlsTunnel {
    proxy_addr: SocketAddr,
    tls_port: u16,
    proxy_task: JoinHandle<(String, Option<String>)>,
    tls_task: JoinHandle<(bool, Option<String>)>,
}

impl TlsTunnel {
    pub(crate) async fn start() -> io::Result<Self> {
        let tls_listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let tls_port = tls_listener.local_addr()?.port();
        let proxy_listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let proxy_addr = proxy_listener.local_addr()?;

        let cert = CertificateDer::from(include_bytes!("../fixtures/tls/localhost.der").to_vec());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            include_bytes!("../fixtures/tls/localhost-key.der").to_vec(),
        ));
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .map_err(io::Error::other)?;
        let acceptor = TlsAcceptor::from(Arc::new(config));

        let tls_task = tokio::spawn(async move {
            let (stream, _) = timeout(IO_TIMEOUT, tls_listener.accept())
                .await
                .expect("TLS listener accept timeout")
                .expect("TLS listener accept");
            let handshake = timeout(IO_TIMEOUT, acceptor.accept(stream))
                .await
                .expect("TLS handshake timeout");
            let Ok(mut stream) = handshake else {
                return (false, None);
            };
            let mut request = Vec::new();
            loop {
                let mut byte = [0_u8; 1];
                let count = timeout(IO_TIMEOUT, stream.read(&mut byte))
                    .await
                    .expect("TLS HTTP request read timeout")
                    .expect("TLS HTTP request read");
                if count == 0 {
                    return (true, None);
                }
                request.push(byte[0]);
                if request.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&request).into_owned();
            let response =
                b"HTTP/1.1 200 OK\r\nContent-Length: 24\r\nConnection: close\r\n\r\ntls tunnel response body";
            timeout(IO_TIMEOUT, stream.write_all(response))
                .await
                .expect("TLS response write timeout")
                .expect("TLS response write");
            (true, Some(request))
        });

        let target = SocketAddr::from(([127, 0, 0, 1], tls_port));
        let proxy_task = tokio::spawn(async move {
            let (mut client, _) = timeout(IO_TIMEOUT, proxy_listener.accept())
                .await
                .expect("proxy listener accept timeout")
                .expect("proxy listener accept");
            let (authority, authorization) = read_connect(&mut client).await.expect("read CONNECT request");
            assert_eq!(
                authorization.as_deref(),
                Some("Basic dHVubmVsLXVzZXI6dHVubmVsLXBhc3N3b3Jk")
            );
            let mut upstream = timeout(IO_TIMEOUT, TcpStream::connect(target))
                .await
                .expect("connect to TLS service timeout")
                .expect("connect to TLS service");
            timeout(
                IO_TIMEOUT,
                client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n"),
            )
            .await
            .expect("write CONNECT success timeout")
            .expect("write CONNECT success");
            timeout(IO_TIMEOUT, tokio::io::copy_bidirectional(&mut client, &mut upstream))
                .await
                .expect("tunnel copy timeout")
                .expect("copy tunnel");
            (authority, authorization)
        });

        Ok(Self {
            proxy_addr,
            tls_port,
            proxy_task,
            tls_task,
        })
    }

    pub(crate) fn proxy_addr(&self) -> SocketAddr {
        self.proxy_addr
    }

    pub(crate) fn tls_port(&self) -> u16 {
        self.tls_port
    }

    pub(crate) async fn finish(mut self) -> TunnelObservation {
        let (connect_authority, proxy_authorization) = timeout(IO_TIMEOUT, &mut self.proxy_task)
            .await
            .expect("proxy task join timeout")
            .expect("proxy task panicked");
        let (tls_handshake_succeeded, application_request) = timeout(IO_TIMEOUT, &mut self.tls_task)
            .await
            .expect("TLS task join timeout")
            .expect("TLS task panicked");
        TunnelObservation {
            connect_authority,
            proxy_authorization,
            tls_handshake_succeeded,
            application_request,
        }
    }
}

impl Drop for TlsTunnel {
    fn drop(&mut self) {
        self.proxy_task.abort();
        self.tls_task.abort();
    }
}

async fn read_connect(stream: &mut TcpStream) -> io::Result<(String, Option<String>)> {
    let request_line = read_line(stream).await?;
    let mut fields = request_line.split_whitespace();
    let method = fields.next().unwrap_or_default();
    let authority = fields.next().unwrap_or_default().to_owned();
    if method != "CONNECT" || authority.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "expected CONNECT request"));
    }
    let mut authorization = None;
    loop {
        let line = read_line(stream).await?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("proxy-authorization") {
                authorization = Some(value.trim().to_owned());
            }
        }
    }
    Ok((authority, authorization))
}

async fn read_line(stream: &mut TcpStream) -> io::Result<String> {
    let mut line = Vec::new();
    loop {
        let mut byte = [0_u8; 1];
        let count = timeout(IO_TIMEOUT, stream.read(&mut byte))
            .await
            .map_err(io::Error::other)??;
        if count == 0 {
            break;
        }
        line.push(byte[0]);
        if line.ends_with(b"\r\n") {
            break;
        }
    }
    String::from_utf8(line).map_err(io::Error::other)
}
