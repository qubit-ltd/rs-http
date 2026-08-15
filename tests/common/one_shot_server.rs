// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! # One-Shot Test Server
//!
//! Provides a minimal one-request HTTP server used in integration tests.

use std::collections::HashMap;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use url::Url;

/// Captured inbound request details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedRequest {
    /// HTTP method.
    pub method: String,
    /// Raw request target (path + query).
    pub target: String,
    /// Lower-cased request headers.
    pub headers: HashMap<String, String>,
    /// Request body bytes.
    pub body: Vec<u8>,
}

/// One chunk in a chunked response plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseChunk {
    /// Delay before writing this chunk.
    pub delay: Duration,
    /// Raw chunk payload.
    pub bytes: Vec<u8>,
}

/// Server response behavior plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponsePlan {
    /// Send a normal fixed-length response immediately.
    Immediate {
        /// HTTP status code.
        status: u16,
        /// Extra headers.
        headers: Vec<(String, String)>,
        /// Response body.
        body: Vec<u8>,
    },

    /// Send a normal fixed-length response with raw header values immediately.
    ImmediateRawHeaders {
        /// HTTP status code.
        status: u16,
        /// Extra headers as raw bytes.
        headers: Vec<(String, Vec<u8>)>,
        /// Response body.
        body: Vec<u8>,
    },

    /// Delay before sending status line and headers.
    DelayedStart {
        /// Delay duration.
        delay: Duration,
        /// HTTP status code.
        status: u16,
        /// Extra headers.
        headers: Vec<(String, String)>,
        /// Response body.
        body: Vec<u8>,
    },

    /// Send part of a fixed-length body and then stall for a while.
    PartialThenDelay {
        /// HTTP status code.
        status: u16,
        /// Extra headers.
        headers: Vec<(String, String)>,
        /// Content-Length declared in response.
        total_length: usize,
        /// Prefix bytes sent immediately.
        prefix: Vec<u8>,
        /// Delay before closing connection.
        delay: Duration,
    },

    /// Send a chunked response with delayed chunks.
    Chunked {
        /// HTTP status code.
        status: u16,
        /// Extra headers.
        headers: Vec<(String, String)>,
        /// Sequence of chunks.
        chunks: Vec<ResponseChunk>,
        /// Whether to send terminating zero chunk.
        finish: bool,
    },
}

/// Handle to a one-shot test server.
#[derive(Debug)]
pub struct OneShotServer {
    base_url: Url,
    request_received_rx: oneshot::Receiver<()>,
    response_started_rx: oneshot::Receiver<()>,
    response_permit_tx: Option<oneshot::Sender<()>>,
    request_rx: oneshot::Receiver<CapturedRequest>,
    join_handle: tokio::task::JoinHandle<()>,
}

/// Handle to a test server that serves a fixed sequence of responses.
#[derive(Debug)]
pub struct MultiShotServer {
    base_url: Url,
    request_rx: oneshot::Receiver<Vec<CapturedRequest>>,
    join_handle: tokio::task::JoinHandle<()>,
}

impl OneShotServer {
    /// Returns the server base URL (e.g. `http://127.0.0.1:12345/`).
    pub fn base_url(&self) -> Url {
        self.base_url.clone()
    }

    /// Waits until the server has received and parsed the request.
    pub async fn wait_until_request_received(&mut self) {
        (&mut self.request_received_rx)
            .await
            .expect("one-shot test server dropped request-received sender");
    }

    /// Waits until the response head and its first available body bytes have
    /// been flushed to the client connection.
    pub async fn wait_until_response_started(&mut self) {
        (&mut self.response_started_rx)
            .await
            .expect("one-shot test server dropped response-started sender");
    }

    /// Allows a blocked one-shot server to start writing its response.
    pub fn allow_response(&mut self) {
        if let Some(sender) = self.response_permit_tx.take() {
            let _ = sender.send(());
        }
    }

    /// Waits for server completion and returns the captured request.
    pub async fn finish(self) -> CapturedRequest {
        let request = self
            .request_rx
            .await
            .expect("one-shot test server dropped request sender");
        self.join_handle
            .await
            .expect("one-shot test server task panicked");
        request
    }
}

impl MultiShotServer {
    /// Returns the server base URL (e.g. `http://127.0.0.1:12345/`).
    pub fn base_url(&self) -> Url {
        self.base_url.clone()
    }

    /// Waits for server completion and returns all captured requests.
    pub async fn finish(self) -> Vec<CapturedRequest> {
        let requests = self
            .request_rx
            .await
            .expect("multi-shot test server dropped request sender");
        self.join_handle
            .await
            .expect("multi-shot test server task panicked");
        requests
    }
}

/// Spawns a one-shot HTTP server with the provided response behavior.
pub async fn spawn_one_shot_server(plan: ResponsePlan) -> OneShotServer {
    spawn_one_shot_server_impl(plan, false).await
}

/// Spawns a one-shot server that waits for [`OneShotServer::allow_response`]
/// after parsing the request and before writing any response bytes.
pub async fn spawn_blocked_one_shot_server(
    plan: ResponsePlan,
) -> OneShotServer {
    spawn_one_shot_server_impl(plan, true).await
}

/// Shared one-shot server construction with optional response gating.
async fn spawn_one_shot_server_impl(
    plan: ResponsePlan,
    block_response: bool,
) -> OneShotServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind one-shot test server");
    let addr = listener
        .local_addr()
        .expect("failed to query one-shot server local address");
    let base_url = Url::parse(&format!("http://{addr}/"))
        .expect("failed to build base URL");
    let (request_received_tx, request_received_rx) = oneshot::channel::<()>();
    let (response_started_tx, response_started_rx) = oneshot::channel::<()>();
    let (response_permit_tx, response_permit_rx) = oneshot::channel::<()>();
    let mut response_permit_rx = block_response.then_some(response_permit_rx);
    let (request_tx, request_rx) = oneshot::channel::<CapturedRequest>();

    let join_handle = tokio::spawn(async move {
        let accept_result = listener.accept().await;
        let (mut stream, _) = match accept_result {
            Ok(result) => result,
            Err(error) => panic!(
                "one-shot test server failed to accept connection: {error}"
            ),
        };

        let request = read_request(&mut stream)
            .await
            .expect("failed to read request in one-shot test server");
        let _ = request_received_tx.send(());
        let _ = request_tx.send(request);

        if let Some(receiver) = response_permit_rx.take() {
            let _ = receiver.await;
        }

        if let Err(error) =
            write_response(&mut stream, plan, Some(response_started_tx)).await
        {
            // Timeout tests intentionally allow client-side early disconnects.
            if !is_expected_client_disconnect(&error) {
                panic!(
                    "failed to write response in one-shot test server: {error}"
                );
            }
        }
    });

    OneShotServer {
        base_url,
        request_received_rx,
        response_started_rx,
        response_permit_tx: block_response.then_some(response_permit_tx),
        request_rx,
        join_handle,
    }
}

/// Spawns a test HTTP server that serves one response plan per request.
pub async fn spawn_multi_shot_server(
    plans: Vec<ResponsePlan>,
) -> MultiShotServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind multi-shot test server");
    let addr = listener
        .local_addr()
        .expect("failed to query multi-shot server local address");
    let base_url = Url::parse(&format!("http://{addr}/"))
        .expect("failed to build base URL");
    let (request_tx, request_rx) = oneshot::channel::<Vec<CapturedRequest>>();

    let join_handle = tokio::spawn(async move {
        let mut handles = Vec::with_capacity(plans.len());
        for (index, plan) in plans.into_iter().enumerate() {
            let accept_result = listener.accept().await;
            let (mut stream, _) = match accept_result {
                Ok(result) => result,
                Err(error) => panic!(
                    "multi-shot test server failed to accept connection: {error}"
                ),
            };

            handles.push(tokio::spawn(async move {
                let request = read_request(&mut stream)
                    .await
                    .expect("failed to read request in multi-shot test server");

                if let Err(error) =
                    write_response(&mut stream, plan, None).await
                {
                    if !is_expected_client_disconnect(&error) {
                        panic!("failed to write response in multi-shot test server: {error}");
                    }
                }
                (index, request)
            }));
        }
        let mut requests = vec![None; handles.len()];
        for handle in handles {
            let (index, request) =
                handle.await.expect("multi-shot request task panicked");
            requests[index] = Some(request);
        }
        let requests = requests
            .into_iter()
            .map(|request| request.expect("multi-shot request missing"))
            .collect();
        let _ = request_tx.send(requests);
    });

    MultiShotServer {
        base_url,
        request_rx,
        join_handle,
    }
}

async fn read_request(
    stream: &mut TcpStream,
) -> std::io::Result<CapturedRequest> {
    let read_timeout = Duration::from_secs(3);
    let mut buffer = Vec::new();
    let header_end_index = loop {
        let mut chunk = [0_u8; 1024];
        let read_size =
            tokio::time::timeout(read_timeout, stream.read(&mut chunk))
                .await
                .map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "timed out while waiting for request headers",
                    )
                })??;
        if read_size == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed before request headers were complete",
            ));
        }
        buffer.extend_from_slice(&chunk[..read_size]);
        if let Some(index) = find_subsequence(&buffer, b"\r\n\r\n") {
            break index + 4;
        }
    };

    let header_bytes = &buffer[..header_end_index];
    let body = buffer[header_end_index..].to_vec();

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

    Ok(CapturedRequest {
        method,
        target,
        headers,
        body,
    })
}

async fn write_response(
    stream: &mut TcpStream,
    plan: ResponsePlan,
    mut response_started_tx: Option<oneshot::Sender<()>>,
) -> std::io::Result<()> {
    match plan {
        ResponsePlan::Immediate {
            status,
            headers,
            body,
        } => {
            write_fixed_response(stream, status, headers, body).await?;
            signal_response_started(&mut response_started_tx);
        }
        ResponsePlan::ImmediateRawHeaders {
            status,
            mut headers,
            body,
        } => {
            if !contains_raw_header(&headers, "Content-Length") {
                headers.push((
                    "Content-Length".to_string(),
                    body.len().to_string().into_bytes(),
                ));
            }
            write_status_and_raw_headers(stream, status, &headers).await?;
            if !body.is_empty() {
                stream.write_all(&body).await?;
            }
            stream.flush().await?;
            signal_response_started(&mut response_started_tx);
        }
        ResponsePlan::DelayedStart {
            delay,
            status,
            headers,
            body,
        } => {
            tokio::time::sleep(delay).await;
            write_fixed_response(stream, status, headers, body).await?;
            signal_response_started(&mut response_started_tx);
        }
        ResponsePlan::PartialThenDelay {
            status,
            mut headers,
            total_length,
            prefix,
            delay,
        } => {
            if !contains_header(&headers, "Content-Length") {
                headers.push((
                    "Content-Length".to_string(),
                    total_length.to_string(),
                ));
            }
            write_status_and_headers(stream, status, &headers).await?;
            stream.write_all(&prefix).await?;
            stream.flush().await?;
            signal_response_started(&mut response_started_tx);
            tokio::time::sleep(delay).await;
        }
        ResponsePlan::Chunked {
            status,
            mut headers,
            chunks,
            finish,
        } => {
            if !contains_header(&headers, "Transfer-Encoding") {
                headers.push((
                    "Transfer-Encoding".to_string(),
                    "chunked".to_string(),
                ));
            }
            write_status_and_headers(stream, status, &headers).await?;

            if chunks.is_empty() {
                stream.flush().await?;
                signal_response_started(&mut response_started_tx);
            }

            for (index, chunk) in chunks.into_iter().enumerate() {
                if !chunk.delay.is_zero() {
                    tokio::time::sleep(chunk.delay).await;
                }
                let length_line = format!("{:X}\r\n", chunk.bytes.len());
                stream.write_all(length_line.as_bytes()).await?;
                stream.write_all(&chunk.bytes).await?;
                stream.write_all(b"\r\n").await?;
                stream.flush().await?;
                if index == 0 {
                    signal_response_started(&mut response_started_tx);
                }
            }

            if finish {
                stream.write_all(b"0\r\n\r\n").await?;
                stream.flush().await?;
            }
        }
    }
    Ok(())
}

/// Signals that the response is observable by the client exactly once.
fn signal_response_started(
    response_started_tx: &mut Option<oneshot::Sender<()>>,
) {
    if let Some(sender) = response_started_tx.take() {
        let _ = sender.send(());
    }
}

async fn write_fixed_response(
    stream: &mut TcpStream,
    status: u16,
    mut headers: Vec<(String, String)>,
    body: Vec<u8>,
) -> std::io::Result<()> {
    if !contains_header(&headers, "Content-Length") {
        headers.push(("Content-Length".to_string(), body.len().to_string()));
    }
    write_status_and_headers(stream, status, &headers).await?;
    if !body.is_empty() {
        stream.write_all(&body).await?;
    }
    stream.flush().await?;
    Ok(())
}

async fn write_status_and_headers(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(String, String)],
) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", status, reason_phrase(status));
    for (name, value) in headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    Ok(())
}

fn contains_header(headers: &[(String, String)], name: &str) -> bool {
    headers
        .iter()
        .any(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
}

fn contains_raw_header(headers: &[(String, Vec<u8>)], name: &str) -> bool {
    headers
        .iter()
        .any(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
}

async fn write_status_and_raw_headers(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(String, Vec<u8>)],
) -> std::io::Result<()> {
    let status_line =
        format!("HTTP/1.1 {} {}\r\n", status, reason_phrase(status));
    stream.write_all(status_line.as_bytes()).await?;
    for (name, value) in headers {
        stream.write_all(name.as_bytes()).await?;
        stream.write_all(b": ").await?;
        stream.write_all(value).await?;
        stream.write_all(b"\r\n").await?;
    }
    stream.write_all(b"\r\n").await?;
    Ok(())
}

/// Returns whether a write failure means the client closed the connection
/// first.
fn is_expected_client_disconnect(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::NotConnected
    )
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}
