# Qubit HTTP (`rs-http`)

[![CircleCI](https://circleci.com/gh/qubit-ltd/rs-http.svg?style=shield)](https://circleci.com/gh/qubit-ltd/rs-http)
[![Coverage Status](https://coveralls.io/repos/github/qubit-ltd/rs-http/badge.svg?branch=main)](https://coveralls.io/github/qubit-ltd/rs-http?branch=main)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
[![Rust](https://img.shields.io/badge/rust-1.70+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

`qubit-http` is a Rust HTTP infrastructure crate for building API clients with consistent behavior:

- one client model for request/response and streaming
- explicit timeout, retry, proxy, and logging controls
- built-in SSE event and JSON chunk decoding
- unified error model (`HttpError`, `HttpErrorKind`, `RetryHint`)

If this is your first time using the crate, start with **Quick Start**, then jump to the scenario you need.

## Installation

```toml
[dependencies]
qubit-http = "0.2.0"
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

    let response = client.execute(request).await?;
    println!("status = {}", response.status);
    println!("text = {}", response.text()?);
    Ok(())
}
```

## Core Types

- `HttpClientFactory`: creates clients from defaults, explicit options, or config.
- `HttpClientOptions`: base URL, default headers, timeouts, proxy, retry, logging, sensitive headers.
- `HttpClient`: executes requests with unified behavior.
- `HttpRequestBuilder`: builds request path/query/headers/body/timeout.
- `HttpResponse`: buffered response (`status`, `headers`, `body`, `text()`, `json()`).
- `HttpStreamResponse`: metadata + streaming body (`into_stream()`).
- `qubit_http::sse`: SSE event parsing and JSON chunk decoding.

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
    println!("status={}", response.status);
    Ok(())
}
```

### 2) Header injection and precedence

Header precedence is:

`default headers` -> `header injectors` -> `request headers` (highest priority)

```rust
use http::HeaderValue;
use qubit_http::{HeaderInjector, HttpClientFactory};

fn with_auth_injector() -> qubit_http::HttpResult<qubit_http::HttpClient> {
    let token = "secret-token".to_string();
    let mut client = HttpClientFactory::new().create()?;

    client.add_header("x-client", "my-app")?;
    client.add_header_injector(HeaderInjector::new(move |headers| {
        let bearer = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|e| qubit_http::HttpError::other(format!("invalid auth header: {e}")))?;
        headers.insert(http::header::AUTHORIZATION, bearer);
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

### 4) Stream response body bytes

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

### 5) Decode SSE JSON chunks

```rust
use futures_util::StreamExt;
use qubit_http::sse::{DoneMarkerPolicy, SseChunk};

#[derive(Debug, serde::Deserialize)]
struct StreamChunk {
    delta: String,
}

async fn consume_sse_json(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let response = client
        .execute_stream(client.request(http::Method::GET, "/v1/stream").build())
        .await?;

    let mut chunks = response.decode_json_chunks::<StreamChunk>(DoneMarkerPolicy::DefaultDone);

    while let Some(item) = chunks.next().await {
        match item? {
            SseChunk::Data(chunk) => println!("delta={}", chunk.delta),
            SseChunk::Done => break,
        }
    }
    Ok(())
}
```

Use `response.decode_json_chunks_with_mode(..., SseJsonMode::Strict)` when malformed JSON should fail immediately.

### 6) OpenAI SSE quick examples (`chat.completions` vs `responses`)

The examples below focus on request + decoding only.
OpenAI payload schemas evolve; refer to the latest API docs:

- Chat Completions streaming events:
  `https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events`
- Responses streaming:
  `https://developers.openai.com/api/reference/resources/responses/methods/create`

```rust
use futures_util::StreamExt;
use http::Method;
use qubit_http::sse::{DoneMarkerPolicy, SseChunk};
use serde_json::json;

// Define these structs in your project according to OpenAI docs:
// - ChatCompletionChunk:
//   https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events
// - ResponseOutputTextDeltaEvent:
//   https://developers.openai.com/api/reference/resources/responses/methods/create

/// Parse OpenAI Chat Completions stream:
/// - data: { "object":"chat.completion.chunk", ... }
/// - data: [DONE]
async fn stream_openai_chat_completions(
    client: &qubit_http::HttpClient,
    api_key: &str,
) -> qubit_http::HttpResult<()> {
    let request = client
        .request(Method::POST, "/v1/chat/completions")
        .header("authorization", &format!("Bearer {api_key}"))?
        .header("accept", "text/event-stream")?
        .json_body(&json!({
            "model": "gpt-4.1-mini",
            "messages": [{"role": "user", "content": "Say hello in Chinese."}],
            "stream": true
        }))?
        .build();

    let response = client.execute_stream(request).await?;
    let mut chunks =
        response.decode_json_chunks::<ChatCompletionChunk>(DoneMarkerPolicy::DefaultDone);

    while let Some(item) = chunks.next().await {
        match item? {
            SseChunk::Data(chunk) => {
                if let Some(text) = chunk
                    .choices
                    .first()
                    .and_then(|choice| choice.delta.content.as_deref())
                {
                    print!("{text}");
                }
            }
            SseChunk::Done => break,
        }
    }
    Ok(())
}

/// Parse OpenAI Responses stream:
/// - event: response.output_text.delta
///   data: { ... "delta": "..." ... }
/// - event: response.completed
async fn stream_openai_responses(
    client: &qubit_http::HttpClient,
    api_key: &str,
) -> qubit_http::HttpResult<()> {
    let request = client
        .request(Method::POST, "/v1/responses")
        .header("authorization", &format!("Bearer {api_key}"))?
        .header("accept", "text/event-stream")?
        .json_body(&json!({
            "model": "gpt-4.1-mini",
            "input": "Give one Rust tip.",
            "stream": true
        }))?
        .build();

    let response = client.execute_stream(request).await?;
    let mut events = response.decode_events();

    while let Some(item) = events.next().await {
        let event = item?;
        match event.event.as_deref() {
            Some("response.output_text.delta") => {
                let data: ResponseOutputTextDeltaEvent = event.decode_json()?;
                print!("{}", data.delta);
            }
            Some("response.completed") => break,
            Some("response.error") => {
                return Err(qubit_http::HttpError::other(format!(
                    "responses error event: {}",
                    event.data
                )));
            }
            _ => {}
        }
    }
    Ok(())
}
```

### 7) Create client from config

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
    config.set("http.retry.enabled", true).unwrap();
    config.set("http.retry.max_attempts", 3_u32).unwrap();

    HttpClientFactory::new().create_from_config(&config.prefix_view("http"))
}
```

## Error Handling

`execute(...)` and `execute_stream(...)` return `Err(HttpError)` for:

- invalid URL resolution
- transport/timeout failures
- non-success HTTP status (`HttpErrorKind::Status`)
- decode/SSE failures

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
        _ => eprintln!("http error: {}", error),
    }

    let retryable = matches!(error.retry_hint(), RetryHint::Retryable);
    eprintln!("retryable={retryable}");

    if let Some(preview) = &error.response_body_preview {
        eprintln!("response preview={preview}");
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
| `logging.enabled` | `true` |
| `logging.*` header/body toggles | all `true` |
| `logging.body_size_limit` | `16 * 1024` bytes |
| `retry.enabled` | `false` |
| `retry.max_attempts` | `3` |
| `retry.delay_strategy` | exponential (`200ms`, `5s`, `2.0`) |
| `retry.jitter_factor` | `0.1` |
| `retry.method_policy` | `IdempotentOnly` |
| `ipv4_only` | `false` |

## Notes

- This crate intentionally focuses on a stable, common HTTP surface rather than exposing the full `reqwest` API.
- For logging output, enable a `tracing` subscriber with `TRACE` level.
- Sensitive headers are masked in logs.
- Binary or non-UTF8 bodies are printed as binary summaries instead of raw bytes.
- `proxy.enabled = false` disables environment proxy inheritance.
- `ipv4_only` enforces IPv4 DNS resolution and rejects IPv6 literal target/proxy hosts.
- Streaming retry covers failures before `HttpStreamResponse` is returned. Errors after streaming starts are surfaced to the caller.

## License

Apache 2.0
