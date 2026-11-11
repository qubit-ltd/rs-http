// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error;
use std::io;
use std::time::Duration;

use http::Method;
use qubit_http::HttpClient;
use qubit_http::HttpClientOptions;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::time::timeout;

const IO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_HEADER_BYTES: usize = 8 * 1024;

async fn serve_one_request(listener: TcpListener) -> io::Result<()> {
    let (mut socket, _) = timeout(IO_TIMEOUT, listener.accept())
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "accept timed out"))??;
    let request = timeout(IO_TIMEOUT, read_request_headers(&mut socket))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "reading request timed out"))??;

    let request_text = String::from_utf8_lossy(&request);
    let mut lines = request_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    if request_line != "GET /anything?from=local-quick-start HTTP/1.1" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected request line: {request_line}"),
        ));
    }
    let has_client_id = lines.any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("x-client-id") && value.trim() == "demo"
        })
    });
    if !has_client_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing x-client-id: demo",
        ));
    }

    socket
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nready")
        .await
}

async fn read_request_headers(socket: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        let count = socket.read(&mut chunk).await?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed before request headers ended",
            ));
        }
        if request.len() + count > MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request headers exceeded 8 KiB",
            ));
        }
        request.extend_from_slice(&chunk[..count]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(request);
        }
    }
}

pub async fn run() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let mut server = tokio::spawn(serve_one_request(listener));

    let client_result = timeout(IO_TIMEOUT, async {
        let mut options = HttpClientOptions::new();
        options.set_base_url(&format!("http://{address}"))?;
        options.add_header("x-client-id", "demo")?;

        let client = HttpClient::builder().options(options).build()?;
        let request = client
            .request(Method::GET, "/anything")
            .query_param("from", "local-quick-start")
            .build();
        let mut response = client.execute(request).await?;
        let status = response.status();
        let text = response.text().await?;
        if status != http::StatusCode::OK {
            return Err(io::Error::other(format!("unexpected status: {status}")).into());
        }
        if text != "ready" {
            return Err(io::Error::other(format!("unexpected response text: {text}")).into());
        }
        println!("status = {status}");
        println!("text = {text}");
        Ok::<(), Box<dyn Error>>(())
    })
    .await;
    let server_result = timeout(IO_TIMEOUT, &mut server).await;
    if server_result.is_err() {
        server.abort();
    }

    client_result??;
    match server_result {
        Ok(join_result) => join_result??,
        Err(_) => {
            let _ = server.await;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "server task did not finish in time",
            )
            .into());
        }
    }
    Ok(())
}

#[cfg(not(test))]
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    run().await
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_run_serves_a_local_request_and_completes() {
        super::run()
            .await
            .expect("local quick start should complete");
    }
}
