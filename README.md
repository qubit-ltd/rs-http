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

A request interceptor’s final method and retry override determine whether an already admitted retry flow may continue. IPv4-only clients reject IPv6-literal redirect targets as `InvalidUrl`. Explicit proxy hosts must contain only a domain or IP literal; keep the port and credentials in their separate options. Multipart `Content-Type` values may contain only one `boundary` parameter, and the body delimiter must match it. For ordinary HTTP retry failures, use `retry_diagnostics()` and the retained `RetryError<HttpError>` source; the legacy `retry_metadata()` getter is deprecated and always empty.

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

This example uses `httpbin.org`, so you can run it without starting a local test server.

```rust
use http::Method;
use qubit_http::{HttpClientBuilder, HttpClientOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = HttpClientOptions::new();
    options.set_base_url("https://httpbin.org")?;
    options.add_header("x-client-id", "demo")?;

    let client = HttpClientBuilder::new().create(options)?;

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

- `qubit-http` is built on top of `reqwest`; it focuses on a stable shared HTTP surface rather than exposing every `reqwest` API.
- Response bodies are read lazily. TRACE response-body logging reports a body only
  when it has already been buffered; it never consumes an unconsumed backend
  stream.
- Built-in request retry covers failures before `HttpResponse` is returned. Stream body errors after return are surfaced to the caller.
- SSE reconnect has a dedicated API: `HttpClient::execute_sse_with_reconnect(...)`.
- Cached response bodies remain reusable: repeated `bytes` / `text` / `json` reads and cached `stream()` calls do not consume the cache.
- The standard redaction policy preserves URL paths; choose strict/path redaction when they may contain secrets. See the [Logging Redaction guide](doc/user_guide.en.md#logging-redaction).
- SSE reconnect has separate cursor, retry-hint, and 204 termination behavior; see [SSE Auto-reconnect](doc/user_guide.en.md#sse-auto-reconnect). Reconnect is not a claim of full EventSource compliance.
- Error preview and retained raw-body limits are independent; a preview cut does not mark a complete raw body truncated. `read_timeout` bounds the complete non-success error-body capture phase; on expiry, the `Status` error retains its status and partial raw body marked incomplete.
- SSE data events dispatch only at a blank line. EOF drops an incomplete frame; servers must terminate the final event with a blank line. One leading BOM is ignored at the start of the stream. `AllowEof` permits complete streams without a done marker but does not relax framing; `RequireDoneMarker` accepts a marker only in a complete event. See [SSE Decoding](doc/user_guide.en.md#sse-decoding) for limits and framing details.
- Automatic SSE decode diagnostics do not include raw event or cursor values; Lenient logs report only presence and byte lengths.
- `SameOrigin` is the default request/redirect policy. Set `origin_policy` to `AnyOrigin` explicitly when cross-origin targets are required; config accepts `same_origin` or `any_origin`.
- JSON SSE consumers can require an explicit done marker with
  `SseCompletionPolicy::RequireDoneMarker`; see the [SSE guide](doc/user_guide.en.md#sse-json-chunks).

See [Automatic Retry](doc/user_guide.en.md#automatic-retry) and [Error Model](doc/user_guide.en.md#error-model)
for retry budgets, timeout stages, replay rules, diagnostics, and the retained
`RetryError<HttpError>` source contract. The response-header deadline starts
after synchronous request interceptors; `request_timeout` covers reqwest's
single-attempt network request and response-body reads.

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
