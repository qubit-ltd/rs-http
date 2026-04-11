# Qubit HTTP (`rust-http`)

[![CircleCI](https://circleci.com/gh/qubit-ltd/rs-http.svg?style=shield)](https://circleci.com/gh/qubit-ltd/rs-http)
[![Coverage Status](https://coveralls.io/repos/github/qubit-ltd/rs-http/badge.svg?branch=main)](https://coveralls.io/github/qubit-ltd/rs-http?branch=main)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
[![Rust](https://img.shields.io/badge/rust-1.70+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

A general-purpose Rust HTTP infrastructure crate with unified client semantics, secure logging, and built-in SSE decoding.

## Features

- Unified HTTP options:
  - `base_url`, `default_headers`, timeouts, proxy, logging, sensitive headers, `ipv4_only`
- Factory abstraction:
  - `HttpClientFactory` (reqwest-backed)
- High-frequency client API:
  - `request(...)`, `execute(...)`, `execute_stream(...)`
- Header convenience methods:
  - `HttpClientOptions::add_header(s)` for pre-create defaults
  - `HttpClient::add_header(s)` for post-create defaults
- Header injection pipeline:
  - `default headers -> injectors -> request headers (override last)`
- Timeout semantics:
  - `connect_timeout` via reqwest
  - `write_timeout` wraps send phase
  - `read_timeout` wraps body/chunk reads
  - optional `request_timeout` as overall reqwest timeout
- Proxy support:
  - `http` / `https` / `socks5`
  - optional proxy auth
  - explicit `proxy.enabled = false` disables environment proxy inheritance
- Logging and masking:
  - request/response header/body toggles
  - sensitive headers are masked (`<=4 => ****`, otherwise keep first 2 + last 2)
  - binary/non-UTF8 body is not printed as raw text
- Built-in SSE decoding (`qubit_http::sse`):
  - line decoding (`\n` / `\r\n`)
  - frame decoding (`data/event/id/retry`)
  - done marker policies (`[DONE]` / custom / disabled)
  - JSON chunk decode with lenient and strict modes
- Unified errors:
  - `HttpError`, `HttpErrorKind`, `RetryHint`

## Installation

```toml
[dependencies]
qubit-http = "0.2.0"
```

## Usage Scenarios

### 1) Basic request (JSON response)

```rust
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = HttpClientOptions::default();
    options.set_base_url("https://example.com")?;
    options.add_header("x-client-id", "demo")?;

    let client = HttpClientFactory::new().create_with_options(options)?;

    let request = client.request(Method::GET, "/health").build();
    let response = client.execute(request).await?;

    println!("status={}", response.status);
    println!("body={}", response.text()?);
    Ok(())
}
```

### 2) Build request with query/header/body

```rust
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};

#[derive(serde::Serialize)]
struct CreateMessageRequest {
    role: String,
    content: String,
}

async fn create_message() -> qubit_http::HttpResult<()> {
    let mut options = HttpClientOptions::default();
    options.set_base_url("https://api.example.com")?;
    options.add_headers([
        ("x-client-id", "demo"),
        ("x-env", "test"),
    ])?;
    let client = HttpClientFactory::new().create_with_options(options)?;

    let body = CreateMessageRequest {
        role: "user".to_string(),
        content: "hello".to_string(),
    };

    let request = client
        .request(Method::POST, "/v1/messages")
        .query_param("stream", "false")
        .header("x-request-id", "req-123")?
        .json_body(&body)?
        .build();

    let response = client.execute(request).await?;
    println!("status={}", response.status);
    Ok(())
}
```

### 3) Header injector (auth / tenant headers)

```rust
use http::HeaderValue;
use qubit_http::{HeaderInjector, HttpClientFactory, HttpClientOptions, HttpResult};

fn build_client_with_injector() -> qubit_http::HttpResult<qubit_http::HttpClient> {
    let token = "secret-token".to_string();
    let mut client = HttpClientFactory::new().create()?;
    client.add_header_injector(HeaderInjector::new(move |headers| {
        let value = HeaderValue::from_str(&format!("Bearer {}", token))
            .map_err(|e| qubit_http::HttpError::other(format!("invalid auth header: {e}")))?;
        headers.insert(http::header::AUTHORIZATION, value);
        headers.insert("x-tenant-id", HeaderValue::from_static("tenant-a"));
        Ok(())
    }));
    Ok(client)
}
```

### 4) Timeouts (global + per-request override)

```rust
use std::time::Duration;

use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};

async fn request_with_timeouts() -> qubit_http::HttpResult<()> {
    let mut options = HttpClientOptions::default();
    options.timeouts.connect_timeout = Duration::from_secs(3);
    options.timeouts.read_timeout = Duration::from_secs(30);
    options.timeouts.write_timeout = Duration::from_secs(15);
    options.timeouts.request_timeout = Some(Duration::from_secs(60));

    let client = HttpClientFactory::new().create_with_options(options)?;

    // Per-request timeout overrides client default request_timeout.
    let request = client
        .request(Method::GET, "https://example.com/slow")
        .timeout(Duration::from_secs(5))
        .build();
    let _ = client.execute(request).await?;
    Ok(())
}
```

### 5) Proxy (HTTP / HTTPS / SOCKS5)

```rust
use qubit_http::{HttpClientFactory, HttpClientOptions, ProxyType};

fn build_client_with_proxy() -> qubit_http::HttpResult<qubit_http::HttpClient> {
    let mut options = HttpClientOptions::default();
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Socks5; // or ProxyType::Http / ProxyType::Https
    options.proxy.host = Some("127.0.0.1".to_string());
    options.proxy.port = Some(1080);
    options.proxy.username = Some("user".to_string());
    options.proxy.password = Some("pass".to_string());

    HttpClientFactory::new().create_with_options(options)
}
```

### 6) Create client from config

```rust
use std::time::Duration;

use qubit_config::Config;
use qubit_http::HttpClientFactory;

fn build_client_from_config() -> Result<qubit_http::HttpClient, qubit_http::HttpConfigError> {
    let mut config = Config::new();
    config.set("http.base_url", "https://api.example.com".to_string()).unwrap();
    config.set("http.timeouts.connect_timeout", Duration::from_secs(3)).unwrap();
    config.set("http.proxy.enabled", false).unwrap();
    config.set("http.logging.enabled", true).unwrap();

    HttpClientFactory::new().create_from_config(&config, "http")
}
```

### 7) Raw streaming bytes

```rust
use futures_util::StreamExt;
use http::Method;

async fn consume_raw_stream(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let request = client.request(Method::GET, "/v1/stream-bytes").build();
    let response = client.execute_stream(request).await?;

    let mut stream = response.into_stream();
    while let Some(item) = stream.next().await {
        let bytes = item?;
        println!("chunk size = {}", bytes.len());
    }
    Ok(())
}
```

### 8) Streaming + SSE JSON chunks (lenient mode)

```rust
use futures_util::StreamExt;
use http::Method;
use qubit_http::sse::{decode_json_chunks, DoneMarkerPolicy, SseChunk};

#[derive(Debug, serde::Deserialize)]
struct StreamChunk {
    delta: String,
}

async fn run_stream(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let request = client.request(Method::GET, "/v1/stream").build();
    let stream_response = client.execute_stream(request).await?;

    let mut chunks = decode_json_chunks::<StreamChunk>(
        stream_response,
        DoneMarkerPolicy::DefaultDone,
    );

    while let Some(item) = chunks.next().await {
        match item? {
            SseChunk::Data(chunk) => {
                println!("delta={}", chunk.delta);
            }
            SseChunk::Done => break,
        }
    }

    Ok(())
}
```

### 9) SSE strict mode (fail fast on malformed JSON)

```rust
use futures_util::StreamExt;
use qubit_http::sse::{decode_json_chunks_with_mode, DoneMarkerPolicy, SseJsonMode};

#[derive(Debug, serde::Deserialize)]
struct Chunk {
    token: String,
}

async fn strict_sse(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let response = client
        .execute_stream(client.request(http::Method::GET, "/v1/stream").build())
        .await?;

    let mut stream = decode_json_chunks_with_mode::<Chunk>(
        response,
        DoneMarkerPolicy::DefaultDone,
        SseJsonMode::Strict,
    );

    while let Some(item) = stream.next().await {
        let _ = item?; // malformed JSON returns HttpErrorKind::SseDecode
    }
    Ok(())
}
```

### 10) Error classification + retry hint

```rust
use qubit_http::{HttpErrorKind, RetryHint};

fn handle_http_error(error: &qubit_http::HttpError) {
    match error.kind {
        HttpErrorKind::Status if error.status.is_some() => {
            eprintln!("status error: {:?}", error.status);
        }
        HttpErrorKind::ReadTimeout | HttpErrorKind::WriteTimeout | HttpErrorKind::ConnectTimeout => {
            eprintln!("timeout: {}", error);
        }
        _ => {
            eprintln!("http error: {}", error);
        }
    }

    let should_retry = matches!(error.retry_hint(), RetryHint::Retryable);
    eprintln!("retryable={should_retry}");
}
```

## Defaults

| Setting | Default |
| --- | --- |
| `connect_timeout` | `10s` |
| `read_timeout` | `120s` |
| `write_timeout` | `120s` |
| `request_timeout` | `None` |
| `proxy.enabled` | `false` |
| `proxy.proxy_type` | `http` |
| `logging.enabled` | `true` |
| `logging.*` header/body toggles | all `true` |
| `logging.body_size_limit` | `16 * 1024` bytes |
| `ipv4_only` | `false` |

## Test Coverage

Tests are under [`tests/`](tests) and cover:

- options defaults and normalization
- header masking behavior
- request builder validation and body encoding
- factory/proxy validation
- proxy integration paths (`http` / `https CONNECT` / `socks5`)
- client execute and execute_stream paths
- status mapping and timeout behavior
- logging policy behavior (toggle/masking/binary/truncation)
- SSE event/frame/JSON decoding behavior

## Current Limitations

- This crate intentionally does not wrap the full `reqwest` API.
- Non-HTTP streaming protocols (WebSocket, gRPC) are out of scope.
- `ipv4_only` is currently a validated option flag; transport-level resolver enforcement is planned as a follow-up enhancement.

## License

Apache 2.0
