# Qubit HTTP (`rs-http`)

[![Rust CI](https://github.com/qubit-ltd/rs-http/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-http/actions/workflows/ci.yml)
[![Coverage Status](https://coveralls.io/repos/github/qubit-ltd/rs-http/badge.svg?branch=main)](https://coveralls.io/github/qubit-ltd/rs-http?branch=main)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
[![Docs.rs](https://docs.rs/qubit-http/badge.svg)](https://docs.rs/qubit-http)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

Documentation: [User Guide](doc/user_guide.en.md) | [API Reference](https://docs.rs/qubit-http)

`qubit-http` is a production-oriented Rust HTTP infrastructure crate for building API clients with consistent behavior across services.

It builds on `reqwest` and provides the common pieces most API clients need: request construction, timeouts, retries, cancellation, streaming responses, SSE, logging, and unified errors.

## Why Use It

Use `qubit-http` when you need:

- one request execution flow for buffered, lazy, and streaming response bodies
- shared timeout, retry, cancellation, proxy, redirect, and logging behavior
- consistent error handling through `HttpError`, `HttpErrorKind`, and `RetryHint`
- built-in helpers for JSON, form, multipart, NDJSON, streaming, and SSE
- config-driven client creation for service-level consistency

For full examples and advanced options, read the [User Guide](doc/user_guide.en.md).

## Installation

```toml
[dependencies]
qubit-http = "0.5"
http = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Some examples in the user guide use optional helper crates such as `serde`, `serde_json`, `futures-util`, and `qubit-config`.

## Quick Start

This example uses `httpbin.org`, so you can run it without starting a local test server.

```rust
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = HttpClientOptions::new();
    options.set_base_url("https://httpbin.org")?;
    options.add_header("x-client-id", "demo")?;

    let client = HttpClientFactory::new().create(options)?;

    let request = client
        .request(Method::GET, "/anything")
        .query_param("from", "readme")
        .build();

    let mut response = client.execute(request).await?;
    println!("status = {}", response.status());
    println!("text = {}", response.text().await?);
    Ok(())
}
```

## Common Next Steps

| Task | Read |
| --- | --- |
| Create clients from defaults, options, or config | [User Guide](doc/user_guide.en.md) |
| Build JSON, form, multipart, NDJSON, or stream request bodies | [User Guide](doc/user_guide.en.md) |
| Add default headers, header injectors, and interceptors | [User Guide](doc/user_guide.en.md) |
| Configure timeouts, retries, cancellation, and `Retry-After` handling | [User Guide](doc/user_guide.en.md) |
| Read bytes, text, JSON, streams, or SSE chunks | [User Guide](doc/user_guide.en.md) |
| Configure logging, sensitive headers, proxy, redirects, and IPv4-only mode | [User Guide](doc/user_guide.en.md) |
| Handle status, transport, timeout, cancellation, decode, and retry errors | [User Guide](doc/user_guide.en.md) |

## Core API At A Glance

| Type | Purpose |
| --- | --- |
| `HttpClientFactory` | Creates clients from defaults, explicit options, or config. |
| `HttpClientOptions` | Holds client-level defaults for base URL, headers, timeouts, retry, logging, proxy, redirects, connection pool, and SSE decoding. |
| `HttpClient` | Executes requests and applies headers, injectors, interceptors, retry, logging, and SSE reconnect helpers. |
| `HttpRequestBuilder` | Builds method, path, query, headers, body, and request-level overrides. |
| `HttpResponse` | Exposes response metadata and lazy readers for bytes, text, JSON, streams, and SSE. |

## Project Scope

- `qubit-http` is built on top of `reqwest`; it focuses on a stable shared HTTP surface rather than exposing every `reqwest` API.
- Response bodies are read lazily unless TRACE response-body logging is enabled.
- Built-in request retry covers failures before `HttpResponse` is returned. Stream body errors after return are surfaced to the caller.
- SSE reconnect has a dedicated API: `HttpClient::execute_sse_with_reconnect(...)`.

## Contributing

Issues and pull requests are welcome.

Please keep contributions focused and easy to review:

- open an issue for bug reports, design questions, or larger feature proposals
- keep pull requests scoped to one behavior change, fix, or documentation update
- follow the [Rust coding style](RUST_CODING_STYLE.md)
- include tests when changing runtime behavior
- update the user guide or README when public API behavior changes

By contributing to this project, you agree that your contribution will be licensed under the same license as the project.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
