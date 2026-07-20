# Qubit HTTP (`rs-http`)

[![Rust CI](https://github.com/qubit-ltd/rs-http/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-http/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-http/coverage-badge.json)](https://qubit-ltd.github.io/rs-http/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
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
qubit-http = "0.10"
qubit-redact = "0.1"
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

## Logging Redaction

Every TRACE and `Debug` path uses one immutable `LogRedactionPolicy` snapshot.
The underlying `qubit-redact` HTTP redactor handles URL userinfo, fragments,
query fields, native-sensitive headers, structured bodies, and hard body
budgets. Non-root URL paths, opaque text, and unkeyed JSON values are redacted
by default.

Build custom policies before installing them on client options:

```rust
use qubit_http::{HttpClientFactory, HttpClientOptions, LogRedactionPolicy};
use qubit_redact::{Sensitivity, http::UrlPathPolicy};

let mut options = HttpClientOptions::new();
options.log_redaction_policy = LogRedactionPolicy::builder()
    .raise_header("x-api-key", Sensitivity::High)
    .raise_query("access_token", Sensitivity::High)
    .raise_body("password", Sensitivity::Secret)
    .allow_query_exact("known_public_token")
    .url_path_policy(UrlPathPolicy::Preserve)
    .build()?;

let client = HttpClientFactory::new().create(options)?;
```

`logging.body_size_limit` is the presentation limit. The policy's
`BodyBudget` remains a second, non-bypassable input/output bound. Truncated
bodies use one generic `<truncated>` marker and retain exact source metadata
when the caller knows it. Configuration uses only the `log_redaction` section;
there is no compatibility path for the old key.

## Common Next Steps

| Task | Read |
| --- | --- |
| Create clients from defaults, options, or config | [User Guide](doc/user_guide.en.md) |
| Build JSON, form, multipart, NDJSON, or stream request bodies | [User Guide](doc/user_guide.en.md) |
| Add default headers, header injectors, and interceptors | [User Guide](doc/user_guide.en.md) |
| Configure timeouts, retries, cancellation, and `Retry-After` handling | [User Guide](doc/user_guide.en.md) |
| Read bytes, text, JSON, streams, or SSE chunks | [User Guide](doc/user_guide.en.md) |
| Configure logging redaction, proxy, redirects, and IPv4-only mode | [User Guide](doc/user_guide.en.md) |
| Handle status, transport, timeout, cancellation, decode, and retry errors | [User Guide](doc/user_guide.en.md) |

## Core API At A Glance

| Type | Purpose |
| --- | --- |
| `HttpClientFactory` | Creates clients from defaults, explicit options, or config. |
| `HttpClientOptions` | Holds client-level defaults for base URL, headers, timeouts, retry, logging, proxy, redirects, connection pool, and SSE decoding. |
| `HttpClient` | Executes requests and applies headers, injectors, interceptors, retry, logging, and SSE reconnect helpers. |
| `HttpRequestBuilder` | Builds method, path, query, headers, body, and request-level overrides. |
| `HttpResponse` | Exposes response metadata and lazy readers for bytes, text, JSON, streams, and SSE. |
| `HttpResponseInterceptorContext` | Lets response interceptors inspect status/method and mutate headers/final URL without breaking success-status invariants. |

## Project Scope

- `qubit-http` is built on top of `reqwest`; it focuses on a stable shared HTTP surface rather than exposing every `reqwest` API.
- Response bodies are read lazily unless TRACE response-body logging is enabled.
- Built-in request retry covers failures before `HttpResponse` is returned. Stream body errors after return are surfaced to the caller.
- SSE reconnect has a dedicated API: `HttpClient::execute_sse_with_reconnect(...)`.

## Testing

```bash
# Run tests with the default feature set
cargo test

# Run tests with all declared features
cargo test --all-features

# Project CI checks
./ci-check.sh

# Check code coverage
./coverage.sh
```

## License

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the
full license text.

## Contributing

Contributions are welcome. Please follow the Rust API guidelines, keep public
API documentation and tests current, and run `./align-ci.sh` to format code and
`./ci-check.sh` to satisfy CI requirements before submitting a pull request.

## Author

**Haixing Hu** - *Qubit Co. Ltd.*

Repository: [https://github.com/qubit-ltd/rs-http](https://github.com/qubit-ltd/rs-http)
