# Qubit HTTP (`rs-http`)

[![Rust CI](https://github.com/qubit-ltd/rs-http/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-http/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-http/coverage-badge.json)](https://qubit-ltd.github.io/rs-http/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

`qubit-http` is a production-oriented Rust HTTP infrastructure crate for building API clients with consistent behavior across services.

It builds on `reqwest` and provides the common pieces most API clients need: request construction, timeouts, retries, cancellation, streaming responses, SSE, logging, and unified errors.

The retry count measures this crate's outer attempts, not wire sends; reqwest's safe protocol recovery remains enabled.

Documentation: [English User Guide](doc/user_guide.en.md) | [中文用户指南](doc/user_guide.zh_CN.md) | [Design](doc/design.md) | [中文设计文档](doc/design.zh_CN.md) | [API Reference](https://docs.rs/qubit-http)


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
qubit-http = "0.16"
qubit-redact = "0.10"
http = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Some examples in the user guide use optional helper crates such as `serde`, `serde_json`, `futures-util`, and `qubit-config`.

## Quick Start

Run the shared local example with `cargo run --example local_quick_start`. It starts a loopback server on an OS-assigned port, sets that address as `base_url`, sends a GET request with a client header and query parameter, then checks the response:

```rust
let mut options = HttpClientOptions::new();
options.set_base_url(&format!("http://{address}"))?;
options.add_header("x-client-id", "demo")?;
let client = HttpClientBuilder::new().create(options)?;
let request = client
    .request(Method::GET, "/anything")
    .query_param("from", "local-quick-start")
    .build();
let mut response = client.execute(request).await?;
println!("status = {}", response.status());
println!("text = {}", response.text().await?);
```

The complete runnable source, including the local server, is [`examples/local_quick_start.rs`](examples/local_quick_start.rs). It prints:

```text
status = 200 OK
text = ready
```

## Logging Redaction

The standard policy preserves URL paths for diagnostics. Use
`RedactionPolicy::strict()` or `UrlPathPolicy::Redact` when paths may contain
opaque identifiers; sensitive query values, headers, and structured body
fields follow their configured rules. `HttpError` applies the same policy to
`Debug` and `Display`. See the [Logging Redaction guide](doc/user_guide.en.md#logging-redaction)
for policy floors, snapshots, budgets, configuration, and examples.

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
| `HttpClientBuilder` | Creates clients from defaults, explicit options, or config. |
| `HttpClientOptions` | Holds client-level defaults for base URL, headers, timeouts, retry, logging, proxy, redirects, connection pool, JSON value limits, and SSE decoding. |
| `HttpClient` | Executes requests and applies headers, injectors, interceptors, retry, logging, and SSE reconnect helpers. |
| `HttpRequestBuilder` | Builds method, path, query, headers, body, and request-level overrides. |
| `HttpResponse` | Exposes response metadata and lazy readers for bytes, text, JSON, streams, and SSE. |
| `HttpResponseInterceptorContext` | Lets response interceptors inspect status/method and mutate headers/final URL without breaking success-status invariants. |

Use `HttpClient::rebuild_with_options` to change client options while retaining registered injectors and interceptors. `to_builder()` copies options only.

## Project Scope

- `qubit-http` provides a stable shared HTTP surface on top of `reqwest`.
- `execute` returns a response only for 2xx status codes. Response bodies are read lazily, and TRACE body logging does not consume an unread backend stream.
- Built-in request retry covers failures before `HttpResponse` is returned; later stream-read errors belong to the caller. SSE reconnect has a separate API and lifecycle. See [Automatic Retry](doc/user_guide.en.md#automatic-retry) and [SSE Auto-reconnect](doc/user_guide.en.md#sse-auto-reconnect).
- `SameOrigin` is the default. Initial URL checks use the request snapshot, while redirect checks use the executing client's backend policy. See [origin policy across clients](doc/user_guide.en.md#origin-policy-across-clients).
- The standard redaction policy preserves URL paths; choose strict or path redaction when paths may contain secrets. See [Logging Redaction](doc/user_guide.en.md#logging-redaction).
- SSE reconnect has dedicated cursor, retry-hint, and 204 termination behavior; it does not claim full EventSource compliance. See [SSE Auto-reconnect](doc/user_guide.en.md#sse-auto-reconnect).
- Error preview, retained raw-body limits, and complete error-body read deadlines are independent. See [Error Model](doc/user_guide.en.md#error-model).

## Testing

```bash
# Run tests with the default feature set
cargo test

# Run tests with all declared features
cargo test --all-features

# Project CI checks
./.infra/bin/ci-check.sh

# Check code coverage
./.infra/bin/coverage.sh
```

## License

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the
full license text.

## Contributing

Contributions are welcome. Please follow the Rust API guidelines, keep public
API documentation and tests current, and run `./.infra/bin/align-ci.sh` to format code and
`./.infra/bin/ci-check.sh` to satisfy CI requirements before submitting a pull request.

## Author

**Haixing Hu** - *Qubit Co. Ltd.*

Repository: [https://github.com/qubit-ltd/rs-http](https://github.com/qubit-ltd/rs-http)
