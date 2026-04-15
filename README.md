# Qubit HTTP (`rs-http`)

[![CircleCI](https://circleci.com/gh/qubit-ltd/rs-http.svg?style=shield)](https://circleci.com/gh/qubit-ltd/rs-http)
[![Coverage Status](https://coveralls.io/repos/github/qubit-ltd/rs-http/badge.svg?branch=main)](https://coveralls.io/github/qubit-ltd/rs-http?branch=main)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
[![Rust](https://img.shields.io/badge/rust-1.70+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

`qubit-http` is a Rust HTTP infrastructure crate for building API clients with consistent behavior:

- one `execute(...)` flow for buffered, lazy, and streaming response bodies
- explicit timeout, retry, proxy, redirect, connection pool, and logging controls
- request-level retry overrides, timeout overrides, base URL overrides, IPv4-only overrides, and cancellation (`CancellationToken`)
- default headers, sync/async header injectors, request interceptors, and response interceptors
- built-in bytes/text/JSON/form/multipart/NDJSON request body helpers
- built-in SSE event decoding, JSON chunk decoding, and reconnect support
- unified error model (`HttpError`, `HttpErrorKind`, `RetryHint`)

If this is your first time using the crate, start with **Quick Start**, then jump to the scenario you need.

## Installation

```toml
[dependencies]
qubit-http = "0.3.1"
http = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
futures-util = "0.3"
qubit-config = "0.8" # optional: only needed for create_from_config(...)
```

## Quick Start

```rust
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = HttpClientOptions::new();
    options.set_base_url("https://httpbin.org")?;
    options.add_header("x-client-id", "demo")?;

    let client = HttpClientFactory::new().create_with_options(options)?;

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

## Core Types

- `HttpClientFactory`: creates clients from defaults, explicit options, or config.
- `HttpClientOptions`: base URL, default headers, timeouts, proxy, retry, logging, sensitive headers, error previews, redirects, connection pool settings, and SSE decoding defaults.
- `HttpClient`: executes requests with unified behavior, supports sync/async header injectors, request interceptors, response interceptors, and SSE reconnect.
- `HttpRequestBuilder`: builds request path/query/headers/body/timeout, request-level retry override, cancellation token, per-request base URL, and IPv4-only override.
- `HttpResponse`: unified response metadata plus a lazily consumed body (`bytes_body().await`, `text().await`, `json().await`, `stream_body()`, `decode_sse_events()`, `decode_sse_json_chunks(...)`).
- `HttpResponseMeta`: response status, headers, final URL, and request method passed to response interceptors.
- `HttpByteStream`: boxed async byte stream returned by `HttpResponse::stream_body()`.
- `qubit_http::sse`: SSE event parsing, JSON chunk decoding, done marker policies, JSON strict/lenient modes, and reconnect options.

## Why This Crate (vs `http` and `reqwest`)

Note: Rust does not ship an HTTP client in the standard library. In practice:
- `http` crate = HTTP types only (`Request`/`Response`/`Method`/`Header`)
- `reqwest` = general-purpose HTTP client
- `qubit-http` = team-oriented HTTP infrastructure built on top of `reqwest`

| Dimension | `http` crate | `reqwest` | `qubit-http` |
| --- | --- | --- | --- |
| Primary role | Type definitions | Generic HTTP client | Unified HTTP infrastructure layer |
| Sends requests | No | Yes | Yes |
| Retry & timeout model | No | Basic capabilities | Unified defaults + request-level override |
| Error semantics | No | `reqwest::Error` centered | `HttpError` + `HttpErrorKind` + `RetryHint` |
| Config consistency | No | App-defined | `HttpClientOptions` + factory + validation |
| Streaming/SSE | No | Raw byte stream | Unified response body stream + SSE event/json decoding + reconnect |
| Observability & safety | No | App-defined | Masking, bounded previews, stable logging behavior |
| Cross-service consistency | Low | Medium | High |

Use `qubit-http` when you want behavior to be consistent across services, not re-implemented per project.

## Common Usage

### 1) Build request with query/header/body

```rust
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};

#[derive(serde::Serialize)]
struct CreateMessageRequest {
    role: String,
    content: String,
}

async fn create_message() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = HttpClientOptions::new();
    options.set_base_url("https://api.example.com")?;

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
    println!("status={}", response.status());
    Ok(())
}
```

### 2) Header injection, interceptors, and precedence

Header precedence is:

`default headers` -> `sync header injectors` -> `async header injectors` -> `request headers` (highest priority)

Request interceptors run before each send attempt and may mutate the request path, query, headers, body, timeouts, or retry override. Response interceptors run on successful HTTP responses before the `HttpResponse` is returned.

```rust
use http::{HeaderValue, StatusCode};
use qubit_http::{
    AsyncHttpHeaderInjector, HttpClientFactory, HttpHeaderInjector, HttpRequestInterceptor,
    HttpResponseInterceptor,
};

fn with_auth_injector() -> qubit_http::HttpResult<qubit_http::HttpClient> {
    let token = "secret-token".to_string();
    let mut client = HttpClientFactory::new().create()?;

    client.add_header("x-client", "my-app")?;
    client.add_header_injector(HttpHeaderInjector::new(move |headers| {
        let bearer = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|e| qubit_http::HttpError::other(format!("invalid auth header: {e}")))?;
        headers.insert(http::header::AUTHORIZATION, bearer);
        Ok(())
    }));
    client.add_async_header_injector(AsyncHttpHeaderInjector::new(|headers| {
        Box::pin(async move {
            headers.insert(
                "x-auth-source",
                HeaderValue::from_static("async-refresh"),
            );
            Ok(())
        })
    }));
    client.add_request_interceptor(HttpRequestInterceptor::new(|request| {
        request.add_query_param("client", "rs-http");
        Ok(())
    }));
    client.add_response_interceptor(HttpResponseInterceptor::new(|meta| {
        if meta.status == StatusCode::NO_CONTENT {
            return Err(qubit_http::HttpError::other("empty response is not accepted"));
        }
        Ok(())
    }));

    Ok(client)
}
```

### 3) Timeouts and retries

```rust
use std::time::Duration;

use http::Method;
use qubit_http::{
    Delay, HttpClientFactory, HttpClientOptions, HttpRetryMethodPolicy,
};

async fn request_with_retry() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = HttpClientOptions::new();
    options.set_base_url("https://api.example.com")?;

    options.timeouts.connect_timeout = Duration::from_secs(3);
    options.timeouts.write_timeout = Duration::from_secs(15);
    options.timeouts.read_timeout = Duration::from_secs(30);
    options.timeouts.request_timeout = Some(Duration::from_secs(60));

    options.retry.enabled = true;
    options.retry.max_attempts = 3;
    options.retry.delay_strategy = Delay::Exponential {
        initial: Duration::from_millis(200),
        max: Duration::from_secs(5),
        multiplier: 2.0,
    };
    options.retry.method_policy = HttpRetryMethodPolicy::IdempotentOnly;

    let client = HttpClientFactory::new().create_with_options(options)?;
    let request = client.request(Method::GET, "/v1/items").build();
    let _ = client.execute(request).await?;
    Ok(())
}
```

Retry applies when:

- `retry.enabled = true`
- `max_attempts > 1`
- method is allowed by `method_policy`
- error is retryable (`timeout`, `transport`, `429`, `5xx`)

### 4) Per-request retry override (`force/disable/method/Retry-After`)

```rust
use http::Method;
use qubit_http::HttpRetryMethodPolicy;

async fn post_with_request_retry_override(
    client: &qubit_http::HttpClient,
) -> qubit_http::HttpResult<()> {
    let request = client
        .request(Method::POST, "/v1/jobs")
        .force_retry()
        .retry_method_policy(HttpRetryMethodPolicy::AllMethods)
        .honor_retry_after(true)
        .json_body(&serde_json::json!({"name": "daily-sync"}))?
        .build();

    let _ = client.execute(request).await?;
    Ok(())
}
```

### 5) Request cancellation (`CancellationToken`)

```rust
use http::Method;
use qubit_http::CancellationToken;

async fn execute_with_cancellation(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let token = CancellationToken::new();
    let token_for_task = token.clone();

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        token_for_task.cancel();
    });

    let request = client
        .request(Method::GET, "/v1/slow-stream")
        .cancellation_token(token)
        .build();
    let _ = client.execute(request).await?;
    Ok(())
}
```

### 6) Form / multipart / NDJSON request body

```rust
use http::Method;
use serde::Serialize;

#[derive(Serialize)]
struct Record {
    id: u64,
}

fn build_body_requests(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let _form = client
        .request(Method::POST, "/v1/form")
        .form_body([("name", "alice"), ("city", "shanghai")])
        .build();

    let boundary = "----qubit-boundary";
    let multipart_payload = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"hello.txt\"\r\n\
         Content-Type: text/plain\r\n\r\nhello\r\n--{boundary}--\r\n"
    );
    let _multipart = client
        .request(Method::POST, "/v1/upload")
        .multipart_body(multipart_payload.into_bytes(), boundary)?
        .build();

    let _ndjson = client
        .request(Method::POST, "/v1/bulk")
        .ndjson_body(&[Record { id: 1 }, Record { id: 2 }])?
        .build();

    Ok(())
}
```

### 7) Stream response body bytes

```rust
use futures_util::StreamExt;
use http::Method;

async fn consume_raw_stream(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let request = client.request(Method::GET, "/v1/stream-bytes").build();
    let mut response = client.execute(request).await?;

    let mut stream = response.stream_body()?;
    while let Some(item) = stream.next().await {
        let bytes = item?;
        println!("chunk size = {}", bytes.len());
    }
    Ok(())
}
```

### 8) Decode SSE JSON chunks

```rust
use futures_util::StreamExt;
use qubit_http::sse::{DoneMarkerPolicy, SseChunk};

#[derive(Debug, serde::Deserialize)]
struct StreamChunk {
    delta: String,
}

async fn consume_sse_json(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let response = client
        .execute(client.request(http::Method::GET, "/v1/stream").build())
        .await?;

    let mut chunks =
        response.decode_sse_json_chunks::<StreamChunk>(DoneMarkerPolicy::DefaultDone);

    while let Some(item) = chunks.next().await {
        match item? {
            SseChunk::Data(chunk) => println!("delta={}", chunk.delta),
            SseChunk::Done => break,
        }
    }
    Ok(())
}
```

You can set client-level SSE defaults via `HttpClientOptions`:

```rust
use qubit_http::sse::SseJsonMode;

options.sse_json_mode = SseJsonMode::Strict;
options.sse_max_line_bytes = 64 * 1024;
options.sse_max_frame_bytes = 1024 * 1024;
```

You can still override per response with `response.decode_sse_json_chunks_with_mode(...)`
or `decode_sse_json_chunks_with_mode_and_limits(...)`:

```rust
use qubit_http::sse::{DoneMarkerPolicy, SseJsonMode};

let chunks = response.decode_sse_json_chunks_with_mode_and_limits::<MyChunk>(
    DoneMarkerPolicy::DefaultDone,
    SseJsonMode::Lenient,
    64 * 1024,   // max_line_bytes
    1024 * 1024, // max_frame_bytes
);
```

### 9) SSE reconnect

```rust
use futures_util::StreamExt;
use http::Method;
use qubit_http::sse::SseReconnectOptions;

async fn consume_sse_with_reconnect(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let request = client
        .request(Method::GET, "/v1/events")
        .header("accept", "text/event-stream")?
        .build();

    let mut events = client.execute_sse_with_reconnect(
        request,
        SseReconnectOptions {
            max_reconnects: 5,
            reconnect_delay: std::time::Duration::from_secs(1),
            reconnect_on_eof: true,
            honor_server_retry: true,
        },
    );

    while let Some(item) = events.next().await {
        let event = item?;
        if let Some(event_name) = event.event.as_deref() {
            println!("event={event_name}");
        }
        println!("data={}", event.data);
    }
    Ok(())
}
```

Reconnect behavior:

- reconnects on retryable transport/read failures
- optionally reconnects on EOF
- sends `Last-Event-ID` from the latest parsed SSE `id:` field
- optionally honors SSE `retry:` as the next reconnect delay

### 10) Create client from config

```rust
use std::time::Duration;

use qubit_config::Config;
use qubit_http::HttpClientFactory;

fn build_client_from_config() -> Result<qubit_http::HttpClient, qubit_http::HttpConfigError> {
    let mut config = Config::new();
    config
        .set("http.base_url", "https://api.example.com".to_string())
        .unwrap();
    config
        .set("http.timeouts.connect_timeout", Duration::from_secs(3))
        .unwrap();
    config.set("http.proxy.enabled", false).unwrap();
    config.set("http.user_agent", "my-service/1.0".to_string()).unwrap();
    config.set("http.max_redirects", 10usize).unwrap();
    config.set("http.retry.enabled", true).unwrap();
    config.set("http.retry.max_attempts", 3_u32).unwrap();
    config
        .set("http.retry.status_codes", vec!["429".to_string(), "503".to_string()])
        .unwrap();
    config.set("http.sse.json_mode", "STRICT".to_string()).unwrap();
    config.set("http.sse.max_line_bytes", 64 * 1024usize).unwrap();
    config
        .set("http.sse.max_frame_bytes", 1024 * 1024usize)
        .unwrap();

    HttpClientFactory::new().create_from_config(&config.prefix_view("http"))
}
```

## Error Handling

`execute(...)`, response body readers, and SSE streams return `Err(HttpError)` for:

- invalid URL resolution
- transport/timeout failures
- non-success HTTP status (`HttpErrorKind::Status`)
- decode/SSE failures
- cancellation (`HttpErrorKind::Cancelled`)

You can inspect `error.kind` and `error.retry_hint()`:

```rust
use qubit_http::{HttpErrorKind, RetryHint};

fn handle_error(error: &qubit_http::HttpError) {
    match error.kind {
        HttpErrorKind::Status => eprintln!("status error: {:?}", error.status),
        HttpErrorKind::ReadTimeout
        | HttpErrorKind::WriteTimeout
        | HttpErrorKind::ConnectTimeout
        | HttpErrorKind::RequestTimeout => {
            eprintln!("timeout: {}", error)
        }
        HttpErrorKind::Cancelled => eprintln!("request cancelled: {}", error),
        _ => eprintln!("http error: {}", error),
    }

    let retryable = matches!(error.retry_hint(), RetryHint::Retryable);
    eprintln!("retryable={retryable}");

    if let Some(preview) = &error.response_body_preview {
        eprintln!("response preview={preview}");
    }
    if let Some(retry_after) = error.retry_after {
        eprintln!("retry-after={retry_after:?}");
    }
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
| `error_response_preview_limit` | `16 * 1024` bytes |
| `user_agent` | `None` |
| `max_redirects` | `None` |
| `pool_idle_timeout` | `None` |
| `pool_max_idle_per_host` | `None` |
| `logging.enabled` | `true` |
| `logging.*` header/body toggles | all `true` |
| `logging.body_size_limit` | `16 * 1024` bytes |
| `retry.enabled` | `false` |
| `retry.max_attempts` | `3` |
| `retry.max_duration` | `None` |
| `retry.delay_strategy` | exponential (`200ms`, `5s`, `2.0`) |
| `retry.jitter_factor` | `0.1` |
| `retry.method_policy` | `IdempotentOnly` |
| `retry.status_codes` | `None` (defaults to `429` and `5xx`) |
| `retry.error_kinds` | `None` (defaults to timeout and transport errors) |
| `ipv4_only` | `false` |
| `sse.json_mode` | `Lenient` |
| `sse.max_line_bytes` | `64 * 1024` bytes |
| `sse.max_frame_bytes` | `1024 * 1024` bytes |

## Notes

- This crate intentionally focuses on a stable, common HTTP surface rather than exposing the full `reqwest` API.
- For logging output, enable a `tracing` subscriber with `TRACE` level.
- Sensitive headers are masked in logs.
- Binary or non-UTF8 bodies are printed as binary summaries instead of raw bytes.
- `proxy.enabled = false` disables environment proxy inheritance.
- `ipv4_only` enforces IPv4 DNS resolution and rejects IPv6 literal target/proxy hosts.
- `create_with_options(...)` always runs `HttpClientOptions::validate()` and fails fast on invalid options.
- `execute(...)` returns after successful response headers and keeps the body lazy unless TRACE response-body logging is enabled.
- Built-in request retry covers failures before `HttpResponse` is returned. Body-read or stream errors after return are surfaced to the caller; use `execute_sse_with_reconnect(...)` for SSE reconnect behavior.

## License

Apache 2.0
