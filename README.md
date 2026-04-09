# Qubit HTTP (`rust-http`)

[![CircleCI](https://circleci.com/gh/qubit-ltd/rust-http.svg?style=shield)](https://circleci.com/gh/qubit-ltd/rust-http)
[![Coverage Status](https://coveralls.io/repos/github/qubit-ltd/rust-http/badge.svg?branch=main)](https://coveralls.io/github/qubit-ltd/rust-http?branch=main)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
[![Rust](https://img.shields.io/badge/rust-1.70+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

A general-purpose Rust HTTP infrastructure crate with unified client semantics, secure logging, and built-in SSE decoding.

## Status

`qubit-http` is implemented and test-covered.

- Implemented: client options, factory, request/response/stream APIs, logging + masking, error model + retry hint, SSE decoding.
- Pending (enhancement): transport-level IPv4-only resolver enforcement.

Design docs:

- PRD: [`doc/http_prd.zh_CN.md`](doc/http_prd.zh_CN.md)
- Design: [`doc/http_design.zh_CN.md`](doc/http_design.zh_CN.md)

## Features

- Unified HTTP options:
  - `base_url`, `default_headers`, timeouts, proxy, logging, sensitive headers, `ipv4_only`
- Factory abstraction:
  - `HttpClientFactory` (reqwest-backed)
- High-frequency client API:
  - `request(...)`, `execute(...)`, `execute_stream(...)`
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
qubit-http = "0.1.0"
```

## Quick Start

```rust
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};
use url::Url;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = HttpClientOptions::default();
    options.base_url = Some(Url::parse("https://example.com")?);

    let client = HttpClientFactory::new().create(options)?;

    let request = client.request(Method::GET, "/health").build();
    let response = client.execute(request).await?;

    println!("status={}", response.status);
    println!("body={}", response.text()?);
    Ok(())
}
```

## Streaming + SSE Example

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

## Retry Hint Integration

```rust
use qubit_http::RetryHint;

fn should_retry(error: &qubit_http::HttpError) -> bool {
    matches!(error.retry_hint(), RetryHint::Retryable)
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
- client execute and execute_stream paths
- status mapping and timeout behavior
- SSE event/frame/JSON decoding behavior

## Current Limitations

- This crate intentionally does not wrap the full `reqwest` API.
- Non-HTTP streaming protocols (WebSocket, gRPC) are out of scope.
- `ipv4_only` is currently a validated option flag; transport-level resolver enforcement is planned as a follow-up enhancement.

## License

Apache 2.0
