# qubit-http User Guide

This guide is based on the current source code and tests. It applies to crate `qubit-http` 0.3.x, imported from Rust code as `qubit_http`.

`qubit-http` is an asynchronous HTTP client infrastructure crate. It wraps `reqwest` and provides unified client options, request building, response reading, error classification, TRACE logging with sensitive-header masking, retries, proxies, IPv4-only resolution, request/response interceptors, and Server-Sent Events (SSE) decoding and reconnection.

## How To Read This Guide

| Goal | Start with |
| --- | --- |
| First integration | “Quick Start”, “Building Requests”, “Reading Responses” |
| Client configuration | “Creating A Client”, “Loading From qubit-config”, “Configuration Reference” |
| Failure diagnosis | “Error Model”, “Automatic Retry”, “Logging And Sensitive Headers” |
| Streaming or SSE | “Reading Responses”, “SSE Decoding” |

## Installation And Imports

```toml
[dependencies]
qubit-http = "0.3"
http = "1.4"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
futures-util = "0.3"
```

```rust
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};
```

## Quick Start

```rust
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct User {
    id: u64,
    name: String,
}

#[tokio::main]
async fn main() -> qubit_http::HttpResult<()> {
    let mut options = HttpClientOptions::new();
    options.set_base_url("https://api.example.com")?;
    options.add_header("x-app", "demo")?;

    let client = HttpClientFactory::new().create(options)?;
    let request = client
        .request(Method::GET, "/users/42")
        .query_param("expand", "profile")
        .build();

    let mut response = client.execute(request).await?;
    let user: User = response.json().await?;
    println!("{user:?}");
    Ok(())
}
```

`execute` treats only 2xx status codes as success. Non-2xx responses return `HttpErrorKind::Status` with status, method, URL, a bounded response-body preview, and a parsed `Retry-After` value when available.

## Creating A Client

### Default Client

```rust
let client = qubit_http::HttpClientFactory::new().create_default()?;
```

Default behavior:

| Item | Default |
| --- | --- |
| `base_url` | None; use absolute URLs or set a request/client base URL |
| Connect timeout | 10 seconds |
| Read timeout | 120 seconds |
| Write timeout | 120 seconds |
| Whole-request timeout | None |
| Proxy | Disabled; calls reqwest `no_proxy()`, so environment proxies are not inherited |
| Logging | Enabled, but logs are emitted only when tracing TRACE is active |
| Log body preview | 16 KiB |
| Non-success response preview | 16 KiB |
| Automatic retry | Disabled |
| Retry max attempts | 3, including the first attempt |
| Retry method policy | Idempotent methods only |
| Sensitive headers | Built-in common auth/token/cookie/secret/password names |
| IPv4-only | Disabled |
| SSE JSON mode | `Lenient` |
| SSE line limit | 64 KiB |
| SSE frame limit | 1 MiB |

### Programmatic Options

```rust
use std::time::Duration;
use qubit_http::{HttpClientFactory, HttpClientOptions, HttpRetryMethodPolicy, RetryDelay};

let mut options = HttpClientOptions::new();
options.set_base_url("https://api.example.com")?;
options.user_agent = Some("my-service/1.0".to_string());
options.max_redirects = Some(5);
options.timeouts.connect_timeout = Duration::from_secs(3);
options.timeouts.request_timeout = Some(Duration::from_secs(30));
options.retry.enabled = true;
options.retry.max_attempts = 4;
options.retry.delay_strategy = RetryDelay::Exponential {
    initial: Duration::from_millis(100),
    max: Duration::from_secs(2),
    multiplier: 2.0,
};
options.retry.method_policy = HttpRetryMethodPolicy::IdempotentOnly;

let client = HttpClientFactory::new().create(options)?;
```

`create` validates options before building the client. Validation includes: all timeout values must be greater than zero; enabled proxies require a non-empty host and non-zero port; a proxy password requires a username; `logging.body_size_limit` must be greater than zero when request or response body logging is enabled; `user_agent` must be non-empty and a valid header value; SSE line and frame limits must be greater than zero.

### Loading From qubit-config

`HttpClientOptions::from_config` and `HttpClientFactory::create_from_config` accept any `qubit_config::ConfigReader`. If you pass `config.prefix_view("http")`, all keys below are read relative to that prefix.

```rust
use std::time::Duration;
use qubit_config::Config;
use qubit_http::HttpClientFactory;

let mut config = Config::new();
config.set("http.base_url", "https://api.example.com".to_string())?;
config.set("http.timeouts.connect_timeout", Duration::from_secs(3))?;
config.set("http.retry.enabled", true)?;
config.set("http.retry.delay_strategy", "FIXED".to_string())?;
config.set("http.retry.fixed_delay", Duration::from_millis(250))?;

let client = HttpClientFactory::new()
    .create_from_config(&config.prefix_view("http"))?;
```

Common configuration keys:

| Key | Description |
| --- | --- |
| `base_url` | Base URL used to resolve relative request paths |
| `timeouts.connect_timeout` | Connect timeout |
| `timeouts.read_timeout` | Per-read wait timeout for body/stream reads |
| `timeouts.write_timeout` | Send-phase timeout |
| `timeouts.request_timeout` | Optional whole-request timeout |
| `proxy.enabled` | Enables outbound proxying |
| `logging.enabled` | Allows TRACE HTTP logs |
| `retry.enabled` | Enables built-in retry |
| `retry.max_attempts` | Max attempts, including the first request |
| `retry.delay_strategy` | `NONE`, `FIXED`, `RANDOM`, `EXPONENTIAL_BACKOFF`, or `EXPONENTIAL` |
| `retry.method_policy` | `IDEMPOTENT_ONLY`/`IDEMPOTENT`, `ALL_METHODS`/`ALL`, or `NONE`/`DISABLED` |
| `retry.status_codes` | Retryable status allowlist; defaults to 429 and 5xx when absent |
| `retry.error_kinds` | Retryable non-status error-kind allowlist; defaults to timeouts and transport when absent |
| `sse.json_mode` | `LENIENT` or `STRICT` |
| `sse.done_marker` | `DISABLED`, `DEFAULT` or `DEFAULT_DONE` map to `DoneMarkerPolicy`; any other non-empty string becomes a `Custom` marker compared to trimmed `data:` text |
| `sse.max_line_bytes` | SSE single-line byte limit |
| `sse.max_frame_bytes` | SSE single-frame byte limit |

See “Configuration Reference” at the end of this guide for the full key list.

`default_headers` supports two forms. The subkey form takes precedence:

```rust
config.set("http.default_headers.authorization", "Bearer token".to_string())?;
config.set("http.default_headers.x-request-id", "abc-123".to_string())?;
```

If there are no `default_headers.*` subkeys, `default_headers` can be a JSON map string:

```rust
config.set(
    "http.default_headers",
    r#"{"x-app-id":"demo","x-version":"1.0"}"#.to_string(),
)?;
```

## Building Requests

Use `client.request(method, path)` to create an `HttpRequestBuilder`. `path` can be an absolute URL or a relative path. Absolute URLs bypass `base_url`; relative paths must be joinable with `base_url`.

```rust
let request = client
    .request(Method::POST, "/events")
    .query_params([("source", "mobile"), ("debug", "false")])
    .header("x-request-id", "req-001")?
    .json_body(&serde_json::json!({"name": "created"}))?
    .request_timeout(Duration::from_secs(10))
    .read_timeout(Duration::from_secs(30))
    .build();
```

Request body builders:

| Method | Behavior |
| --- | --- |
| `bytes_body` | Raw bytes; does not set `Content-Type` |
| `stream_body` | Sends ordered byte chunks through reqwest streaming body support |
| `text_body` | Text body; sets `text/plain; charset=utf-8` when `Content-Type` is absent |
| `json_body` | Serializes JSON; sets `application/json` when `Content-Type` is absent |
| `form_body` | `application/x-www-form-urlencoded` |
| `multipart_body` | Raw multipart bytes; requires non-empty boundary and sets multipart content type when absent |
| `ndjson_body` | One JSON record per line; sets `application/x-ndjson` when absent |

Per-request overrides:

| Method | Purpose |
| --- | --- |
| `request_timeout` | Overrides whole-request timeout (reqwest per-request deadline) |
| `write_timeout` | Overrides send-phase timeout |
| `read_timeout` | Overrides response body/stream read timeout |
| `base_url` / `clear_base_url` | Overrides or removes base URL for this request |
| `ipv4_only` | Overrides IPv4-only URL validation for this request |
| `cancellation_token` | Binds a `CancellationToken`, checked before send, during send, and during body/stream reads |
| `force_retry` | Force-enables retry for this request |
| `disable_retry` | Disables retry for this request |
| `retry_method_policy` | Overrides retryable HTTP method policy for this request |
| `honor_retry_after` | Honors `Retry-After` for retryable 429/5xx responses on this request |

## Headers, Injectors, And Interceptors

Final request headers are merged in this order; later steps override duplicate names:

1. Client default headers snapshotted when the builder is created.
2. Synchronous `HttpHeaderInjector`s in registration order.
3. Asynchronous `AsyncHttpHeaderInjector`s in registration order.
4. Request-local headers.

```rust
use http::{HeaderMap, HeaderName, HeaderValue};
use qubit_http::{AsyncHttpHeaderInjector, HttpHeaderInjector};

client.add_header("x-client", "default")?;

client.add_header_injector(HttpHeaderInjector::new(|headers: &mut HeaderMap| {
    headers.insert(
        HeaderName::from_static("x-sync-token"),
        HeaderValue::from_static("sync-value"),
    );
    Ok(())
}));

client.add_async_header_injector(AsyncHttpHeaderInjector::new(|headers| {
    Box::pin(async move {
        headers.insert(
            HeaderName::from_static("x-async-token"),
            HeaderValue::from_static("async-value"),
        );
        Ok(())
    })
}));
```

Async injectors are useful for headers that must be resolved right before send time, such as authentication tokens. In the example below, `TokenProvider` reuses a cached token while it is still fresh, refreshes it asynchronously when needed, and writes the latest value to `Authorization`. When automatic retry is enabled, async injectors run before each send attempt, so retried requests can pick up a refreshed token too.

```rust
use std::sync::Arc;
use std::time::{Duration, Instant};

use http::header::{AUTHORIZATION, HeaderValue};
use qubit_http::{AsyncHttpHeaderInjector, HttpError, HttpResult};
use tokio::sync::RwLock;

struct CachedToken {
    value: String,
    expires_at: Instant,
}

struct TokenProvider {
    cached: RwLock<Option<CachedToken>>,
}

impl TokenProvider {
    fn new() -> Self {
        Self {
            cached: RwLock::new(None),
        }
    }

    async fn bearer_token(&self) -> HttpResult<String> {
        if let Some(token) = self.cached.read().await.as_ref() {
            if token.expires_at > Instant::now() + Duration::from_secs(30) {
                return Ok(token.value.clone());
            }
        }

        let value = refresh_access_token().await?;
        let expires_at = Instant::now() + Duration::from_secs(3600);
        *self.cached.write().await = Some(CachedToken {
            value: value.clone(),
            expires_at,
        });
        Ok(value)
    }
}

async fn refresh_access_token() -> HttpResult<String> {
    // Call your auth service, read secure storage, or run any other async refresh flow.
    Ok("fresh-token".to_string())
}

let provider = Arc::new(TokenProvider::new());
client.add_async_header_injector(AsyncHttpHeaderInjector::new(move |headers| {
    let provider = Arc::clone(&provider);
    Box::pin(async move {
        let token = provider.bearer_token().await?;
        let value = HeaderValue::from_str(&format!("Bearer {token}")).map_err(|error| {
            HttpError::other(format!("invalid authorization header: {error}"))
        })?;
        headers.insert(AUTHORIZATION, value);
        Ok(())
    })
}));
```

Request interceptors run before each send attempt. They can mutate `HttpRequest`; returning an error short-circuits execution. Response interceptors run only for successful-status responses. They can inspect or mutate `HttpResponseMeta`; returning an error makes `execute` fail.

```rust
use http::{HeaderName, HeaderValue};
use qubit_http::{HttpRequestInterceptor, HttpResponseInterceptor, HttpError};

client.add_request_interceptor(HttpRequestInterceptor::new(|request| {
    request.add_query_param("from_interceptor", "true");
    request.set_typed_header(
        HeaderName::from_static("x-intercepted"),
        HeaderValue::from_static("yes"),
    );
    Ok(())
}));

client.add_response_interceptor(HttpResponseInterceptor::new(|meta| {
    if !meta.headers.contains_key("x-required") {
        return Err(HttpError::other("missing x-required response header"));
    }
    Ok(())
}));
```

## Reading Responses

`HttpResponse` exposes `meta()`, `status()`, `headers()`, `url()`, `request_url()`, `is_success()`, `retry_after_hint()`, and helpers to read the body or decode SSE. After `let mut response = client.execute(...).await?`, pick **one** consumption strategy from the table below; **only one body path applies per `HttpResponse`** (see the note after the table). The **Examples** subsection right after the table shows typical usage for each API listed there.

Body APIs:

| Method | Behavior |
| --- | --- |
| `bytes()` | Lazily reads the full body on the first `await`, caches it, then reuses the cache |
| `text()` | Full-body UTF-8 decode via `bytes()` |
| `json<T>()` | Full-body JSON via `bytes()` |
| `stream()` | Returns `HttpByteStream`; if the body is cached, yields a one-chunk in-memory stream. Otherwise the call does **not** eagerly read the entire body: it hands the backend response to the returned stream, and bytes are read **while that stream is polled** |
| `sse_events()` | Consumes `self` and decodes the body stream into an SSE event stream using the configured line/frame limits (see “SSE Decoding” below) |
| `sse_chunks::<T>()` | Consumes `self` and decodes SSE JSON `data:` chunks into a `SseChunk<T>` stream (JSON mode, done-marker policy, etc.—see “SSE JSON Chunks” below) |

### Examples

**Whole-body reads (`bytes` / `text` / `json`)**: each loads the entire body into the cache. The snippet uses **three separate requests** so each line is valid; do not chain conflicting readers on the same response.

```rust
use http::Method;
use serde::Deserialize;

#[derive(Deserialize)]
struct VersionInfo {
    version: String,
}

async fn read_whole_body_examples(
    client: &qubit_http::HttpClient,
) -> qubit_http::HttpResult<()> {
    let mut r1 = client
        .execute(client.request(Method::GET, "/bytes").build())
        .await?;
    let _raw = r1.bytes().await?;

    let mut r2 = client
        .execute(client.request(Method::GET, "/hello.txt").build())
        .await?;
    let _text = r2.text().await?;

    let mut r3 = client
        .execute(client.request(Method::GET, "/version.json").build())
        .await?;
    let _json: VersionInfo = r3.json().await?;
    Ok(())
}
```

**Streaming bytes (`stream`)**: poll the returned stream chunk by chunk, as long as you have not already consumed the body with `bytes` / `text` / `json` on that same response. `?` applies when obtaining the stream (e.g. if the backend connection is already gone).

```rust
use futures_util::StreamExt;
use http::Method;

async fn read_stream_example(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let mut response = client
        .execute(client.request(Method::GET, "/stream-bytes").build())
        .await?;

    let mut stream = response.stream()?;
    while let Some(item) = stream.next().await {
        let chunk = item?;
        let _len = chunk.len();
    }
    Ok(())
}
```

**SSE (`sse_events` / `sse_chunks`)**: both **consume** `self` (move the response). To tweak line/frame limits, JSON mode, or done-marker policy before decoding, chain the `sse_*` configuration methods with `sse_events()` or `sse_chunks::<T>()` on the same expression (full details under “SSE Decoding” and “SSE JSON Chunks” below).

```rust
use futures_util::StreamExt;
use http::Method;
use qubit_http::sse::SseChunk;

#[derive(Debug, serde::Deserialize)]
struct Delta {
    text: String,
}

async fn read_sse_examples(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let mut ev = client
        .execute(client.request(Method::GET, "/events").build())
        .await?;
    let mut events = ev.sse_events();
    while let Some(item) = events.next().await {
        let _event = item?;
    }

    let mut jc = client
        .execute(client.request(Method::GET, "/chat-stream").build())
        .await?;
    let mut chunks = jc.sse_chunks::<Delta>();
    while let Some(item) = chunks.next().await {
        match item? {
            SseChunk::Data(_d) => {}
            SseChunk::Done => break,
        }
    }
    Ok(())
}
```

There is only one consumption path for the underlying `reqwest` body on a given `HttpResponse`. After `bytes`, `text`, or `json`, the full payload is buffered and `stream` becomes a one-chunk stream over that cache. If you call `stream` first while the body is not cached, the backend handle moves into that stream—you must finish reading there; a later `bytes` / `text` / `json` will not re-read from the network (you get an empty body), so do not mix “stream first, then full-body read” on the same response. `sse_events` / `sse_chunks` also use that path (they build on `stream`) and they **move** the `HttpResponse`, so you cannot call other body readers on the same value afterward.

| Safe | Avoid |
| --- | --- |
| Use `bytes()` / `text()` / `json()` to read and reuse the cached full body | Call `stream()` first, then call `bytes()` / `text()` / `json()` on the same response |
| Call `stream()` after `bytes()` when a one-chunk cached stream is acceptable | Call `sse_events()` or `sse_chunks()` and then try to read the same response body again |
| Chain `sse_*` option setters with the SSE consumer on the same expression | Design multiple body-consumption paths for one `HttpResponse` |

`retry_after_hint()` returns a delay when the response status is 429 or 5xx and the response has a valid `Retry-After` header. It supports both `delta-seconds` and HTTP-date formats; HTTP dates in the past resolve to 0 seconds. `HttpResponseMeta` exposes the same method, so response interceptors can read the hint from metadata.

## Error Model

Runtime HTTP errors use `HttpError`; the result alias is `HttpResult<T> = Result<T, HttpError>`.

`HttpError` contains:

| Field | Meaning |
| --- | --- |
| `kind` | Error category |
| `method` | Optional HTTP method |
| `url` | Optional request URL |
| `status` | Optional response status |
| `message` | Human-readable message |
| `response_body_preview` | Non-2xx response body preview |
| `retry_after` | Parsed `Retry-After` delay |
| `source` | Underlying error |

Error categories:

| Group | Error kinds | Typical meaning |
| --- | --- | --- |
| URL / configuration | `InvalidUrl`, `BuildClient`, `ProxyConfig` | URL resolution failed, client construction failed, or proxy options are invalid |
| Timeout / network | `ConnectTimeout`, `ReadTimeout`, `WriteTimeout`, `RequestTimeout`, `Transport` | Connect/read/write/whole-request timeout or lower-level transport failure |
| HTTP status | `Status` | A non-2xx status code was returned |
| Decoding / SSE | `Decode`, `SseProtocol`, `SseDecode` | Body decoding failed, SSE framing was invalid, or an SSE JSON chunk could not be decoded |
| Retry layer | `RetryAttemptTimeout`, `RetryMaxElapsedExceeded`, `RetryAborted` | The retry executor produced an attempt timeout, elapsed-budget failure, or policy abort |
| Cancellation / fallback | `Cancelled`, `Other` | The request was cancelled, or the failure does not fit another category |

`RetryAttemptTimeout` means one retry-layer attempt exceeded its attempt timeout. `RetryMaxElapsedExceeded` means the total retry elapsed budget was exhausted before a retryable failure was captured. `RetryAborted` means the `qubit-retry` decider stopped early because the current error was not retryable; the original `HttpError` is retained as `source`.

`retry_hint()` marks timeouts, transport errors, 429, and 5xx statuses as retryable hints. The new retry-layer error categories are non-retryable themselves. Actual retry behavior still depends on `HttpRetryOptions` and the method policy.

## Automatic Retry

Automatic retry is disabled by default. When enabled, `execute` enters the retry flow only when `retry.enabled = true`, `max_attempts > 1`, and the method policy allows the current HTTP method.

Default retryability:

| Type | Default retryable values |
| --- | --- |
| Status | 429 and all 5xx |
| Non-status errors | connect/read/write/request timeout and transport |
| Methods | GET, HEAD, PUT, DELETE, OPTIONS, TRACE |

You can configure `retry.status_codes` and `retry.error_kinds` allowlists. Once an allowlist is set, only listed statuses or error kinds are retried.

`retry.error_kinds` accepts config names for every `HttpErrorKind`, including `RETRY_ATTEMPT_TIMEOUT`, `RETRY_MAX_ELAPSED_EXCEEDED`, and `RETRY_ABORTED`. Values are case-insensitive, and hyphens are normalized to underscores.

Per-request override example:

```rust
let request = client
    .request(Method::POST, "/submit")
    .force_retry()
    .retry_method_policy(qubit_http::HttpRetryMethodPolicy::AllMethods)
    .honor_retry_after(true)
    .build();
```

`honor_retry_after(true)` is request-level. For retryable 429 or 5xx responses, if `Retry-After` is present, the retry executor waits at least that duration before the next attempt; if the executor's planned backoff is already longer, no extra delay is added.

When retry is enabled, `execute` runs attempts through `qubit-retry`'s `RetryExecutor`. Retryable failures that exhaust `max_attempts` or `max_duration` return the last HTTP error with exhaustion context appended to `message`. If the current error does not match the active allowlist or retry policy, the executor returns `RetryAborted` and keeps the aborted original `HttpError` as `source`.

| Scenario | Returned error | Notes |
| --- | --- | --- |
| Method policy does not allow replay, such as POST under the default policy | Original single-attempt error | Retry flow is not entered |
| Retry flow is active, but the current error is not retryable | `RetryAborted` | The original `HttpError` is stored in `source` |
| Retryable failure exhausts `max_attempts` | Last `HttpError` | `message` includes attempts-exhausted context |
| Retryable failure exhausts `max_duration` | Last `HttpError` or `RetryMaxElapsedExceeded` | Returns the last error if one was captured; otherwise returns `RetryMaxElapsedExceeded` |

To inspect the original status or error kind from `RetryAborted`, downcast the source:

```rust
if error.kind == qubit_http::HttpErrorKind::RetryAborted {
    if let Some(source) = error.source.as_deref() {
        if let Some(inner) = source.downcast_ref::<qubit_http::HttpError>() {
            eprintln!("original kind={:?}, status={:?}", inner.kind, inner.status);
        }
    }
}
```

## Logging And Sensitive Headers

HTTP logs use `tracing::trace!`. Both conditions must be true:

1. `options.logging.enabled = true`.
2. The active tracing subscriber enables TRACE.

Request headers, request body, response headers, and response body can be toggled separately. Body logs include only the first `logging.body_size_limit` bytes and show a truncation marker for the remainder. Binary bodies are rendered as `<binary N bytes>`.

Sensitive headers are masked. The default set includes common auth, token, cookie, secret, and password header names. Short values are rendered as `****`; longer values keep the first and last 2 characters and replace the middle with `****`. Configuring `sensitive_headers` replaces the default set; code can also manage a `SensitiveHttpHeaders` set directly.

Important: if TRACE logging is active and `log_response_body = true`, `execute` reads and caches the full response body before returning. Such a response is no longer an untouched backend stream.

## Proxy And IPv4-only

Proxying is disabled by default. When disabled, the client calls `no_proxy()`, so environment proxy variables are ignored. Enabled proxies require host and port.

```rust
use qubit_http::{ProxyType, HttpClientOptions};

let mut options = HttpClientOptions::new();
options.proxy.enabled = true;
options.proxy.proxy_type = ProxyType::Socks5;
options.proxy.host = Some("127.0.0.1".to_string());
options.proxy.port = Some(1080);
```

`ProxyType::Socks5` uses scheme `socks5h`, which performs remote DNS. Setting username enables proxy Basic Auth; setting password without username fails validation.

When `ipv4_only = true`:

- the reqwest DNS resolver returns only IPv4 addresses;
- URLs with IPv6 literal hosts are rejected during URL resolution;
- proxy hosts that are IPv6 literals are rejected.

## SSE Decoding

Choose the SSE API by what you need:

| Goal | Use |
| --- | --- |
| Read raw SSE events | `response.sse_events()` |
| Read OpenAI-style JSON chunks or a `[DONE]` marker | `response.sse_chunks::<T>()` |
| Reconnect automatically after a long-lived stream drops | `client.execute_sse_with_reconnect(...)` |

SSE event decoding starts from `HttpResponse`:

```rust
use futures_util::StreamExt;

let response = client
    .execute(client.request(Method::GET, "/stream").build())
    .await?;

let mut events = response.sse_events();
while let Some(item) = events.next().await {
    let event = item?;
    println!("event={:?} id={:?} data={}", event.event, event.id, event.data);
}
```

### Configure `sse_events` options

`sse_max_line_bytes` and `sse_max_frame_bytes` each return `HttpResponse` (moving `self` back to the caller), so you can chain them with `sse_events()` **before** consuming the body. `sse_events()` decodes from `stream()` using the limits stored on that `HttpResponse` instance:

```rust
use futures_util::StreamExt;
use http::Method;

let response = client
    .execute(client.request(Method::GET, "/stream").build())
    .await?;

let mut events = response
    .sse_max_line_bytes(64 * 1024)      // max bytes per SSE line
    .sse_max_frame_bytes(1024 * 1024) // max bytes per SSE frame
    .sse_events();
while let Some(item) = events.next().await {
    let event = item?;
    println!("event={:?} id={:?} data={}", event.event, event.id, event.data);
}
```

`SseEvent` fields:

| Field | Meaning |
| --- | --- |
| `event` | Optional `event:` field |
| `data` | Multiple `data:` lines joined with `\n` |
| `id` | Optional `id:` field |
| `retry` | Optional valid `retry:` value in milliseconds |

Protocol behavior:

- Splits by `\n` and strips trailing `\r`.
- Each line must be UTF-8, otherwise `HttpErrorKind::SseProtocol` is returned.
- A blank line flushes one event.
- Comment lines starting with `:` are ignored.
- Unknown fields are ignored.
- `retry:` is stored only when it parses as `u64`.
- If the stream ends with pending fields, the final event is emitted.
- Default line/frame limits come from `HttpClientOptions`. To override them for one response only, chain `sse_max_line_bytes` / `sse_max_frame_bytes` with `sse_events()` on the same expression, as shown under “Configure `sse_events` options” above.

### SSE JSON Chunks

The following snippets assume `request` is already built, and that `MyChunk` and `handle` are defined by the caller.

`sse_chunks` takes no arguments: the done-marker policy defaults to `DoneMarkerPolicy::DefaultDone` (the `Default` for `DoneMarkerPolicy`), and can be overridden via `HttpClientOptions::sse_done_marker_policy` or `HttpResponse::sse_done_marker_policy`.

```rust
use futures_util::StreamExt;
use qubit_http::sse::SseChunk;

let response = client.execute(request).await?;
let mut chunks = response.sse_chunks::<MyChunk>();

while let Some(item) = chunks.next().await {
    match item? {
        SseChunk::Data(data) => handle(data),
        SseChunk::Done => break,
    }
}
```

### Configure `sse_chunks` options

`sse_json_mode`, `sse_done_marker_policy`, `sse_max_line_bytes`, and `sse_max_frame_bytes` all return `HttpResponse` (moving `self` back to the caller), so you can chain them with `sse_chunks::<T>()` on the same expression. `sse_chunks` reads the effective options from that same instance:

```rust
use futures_util::StreamExt;
use qubit_http::sse::{DoneMarkerPolicy, SseChunk, SseJsonMode};

let response = client.execute(request).await?;

let mut chunks = response
    .sse_json_mode(SseJsonMode::Strict)
    .sse_done_marker_policy(DoneMarkerPolicy::DefaultDone)
    .sse_max_line_bytes(256)
    .sse_max_frame_bytes(16 * 1024)
    .sse_chunks::<MyChunk>();

while let Some(item) = chunks.next().await {
    match item? {
        SseChunk::Data(data) => handle(data),
        SseChunk::Done => break,
    }
}
```

`DoneMarkerPolicy` supports:

- `Disabled`: no done-marker recognition.
- `DefaultDone`: when trimmed `data:` equals `[DONE]`, emits `SseChunk::Done` and ends.
- `Custom(String)`: uses a custom done marker.

`SseJsonMode::Lenient` skips malformed JSON chunks and continues. `Strict` returns `HttpErrorKind::SseDecode` on the first malformed JSON chunk. Configure defaults on `HttpClientOptions` (and under `sse.*` in config), or override for one response by chaining `sse_json_mode`, `sse_done_marker_policy`, `sse_max_line_bytes`, and `sse_max_frame_bytes` with `sse_chunks::<T>()` on the same expression, as under “Configure `sse_chunks` options” above.

### SSE Auto-reconnect

```rust
use futures_util::StreamExt;
use qubit_http::sse::SseReconnectOptions;
use qubit_http::{RetryDelay, RetryJitter, RetryOptions};

let request = client.request(Method::GET, "/events").build();
let mut events = client.execute_sse_with_reconnect(
    request,
    SseReconnectOptions {
        retry: RetryOptions::new(
            6, // max_attempts = initial connect + 5 reconnects
            None,
            RetryDelay::exponential(
                std::time::Duration::from_secs(1),
                std::time::Duration::from_secs(30),
                2.0,
            ),
            RetryJitter::None,
        ).expect("valid SSE retry options"),
        reconnect_on_eof: true,
        honor_server_retry: true,
    },
);

while let Some(item) = events.next().await {
    let event = item?;
    println!("{}", event.data);
}
```

The default reconnect settings are `retry.max_attempts = 4` (that is, at most 3 reconnects), `RetryDelay::Exponential { initial=1s, max=30s, multiplier=2.0 }`, `RetryJitter::None`, reconnect on EOF, and honor server `retry:`. Reconnects reuse the original request. If a previous SSE event had an `id:`, the next request includes `Last-Event-ID`. Cancellation does not reconnect. SSE protocol errors do not reconnect by default. Retryable timeout, transport, 429/5xx, and unexpected-EOF-like errors may reconnect.

## Configuration Reference

The table below lists every configuration key supported by `HttpClientOptions::from_config`. If you pass `config.prefix_view("http")`, these keys are read relative to that prefix.

| Key | Description |
| --- | --- |
| `base_url` | Base URL used to resolve relative request paths |
| `ipv4_only` | Keeps only IPv4 DNS results and rejects IPv6 literal URLs |
| `error_response_preview_limit` | Body preview byte limit stored on non-2xx errors |
| `user_agent` | Default User-Agent passed to the reqwest builder |
| `max_redirects` | Redirect limit |
| `pool_idle_timeout` | Connection pool idle timeout |
| `pool_max_idle_per_host` | Max idle connections per host |
| `sensitive_headers` | String list that replaces the default sensitive-header set |
| `timeouts.connect_timeout` | Connect timeout |
| `timeouts.read_timeout` | Per-read wait timeout for body/stream reads |
| `timeouts.write_timeout` | Send-phase timeout |
| `timeouts.request_timeout` | Optional whole-request timeout |
| `proxy.enabled` | Enables outbound proxying |
| `proxy.proxy_type` | `http`, `https`, `socks5`, or `socks5h` |
| `proxy.host` | Proxy host |
| `proxy.port` | Proxy port |
| `proxy.username` | Proxy Basic Auth username |
| `proxy.password` | Proxy Basic Auth password; requires username |
| `logging.enabled` | Allows TRACE HTTP logs |
| `logging.log_request_header` | Logs request headers |
| `logging.log_request_body` | Logs request body preview |
| `logging.log_response_header` | Logs response headers |
| `logging.log_response_body` | Logs response body preview |
| `logging.body_size_limit` | Log body preview byte limit |
| `retry.enabled` | Enables built-in retry |
| `retry.max_attempts` | Max attempts, including the first request |
| `retry.max_duration` | Optional total retry duration limit |
| `retry.delay_strategy` | `NONE`, `FIXED`, `RANDOM`, `EXPONENTIAL_BACKOFF`, or `EXPONENTIAL` |
| `retry.fixed_delay` | Fixed retry delay |
| `retry.random_min_delay` | Random delay lower bound |
| `retry.random_max_delay` | Random delay upper bound |
| `retry.backoff_initial_delay` | Exponential backoff initial delay |
| `retry.backoff_max_delay` | Exponential backoff max delay |
| `retry.backoff_multiplier` | Exponential backoff multiplier |
| `retry.jitter_factor` | Jitter factor in range `0.0..=1.0` |
| `retry.method_policy` | `IDEMPOTENT_ONLY`/`IDEMPOTENT`, `ALL_METHODS`/`ALL`, or `NONE`/`DISABLED` |
| `retry.status_codes` | Retryable status allowlist; defaults to 429 and 5xx when absent |
| `retry.error_kinds` | Retryable non-status error-kind allowlist; defaults to timeouts and transport when absent |
| `sse.json_mode` | `LENIENT` or `STRICT` |
| `sse.done_marker` | `DISABLED`, `DEFAULT` or `DEFAULT_DONE` map to `DoneMarkerPolicy`; any other non-empty string becomes a `Custom` marker compared to trimmed `data:` text |
| `sse.max_line_bytes` | SSE single-line byte limit |
| `sse.max_frame_bytes` | SSE single-frame byte limit |

## Practical Advice

- Enable global retry for read-only or idempotent APIs. Use `AllMethods` or per-request force retry for POST/PATCH only when the operation is safe to replay.
- Set a realistic `read_timeout` for long-lived streams/SSE; too short a value turns a slow but healthy stream into `ReadTimeout`.
- Disable `logging.log_response_body` when you need true streaming consumption, because TRACE response-body logging pre-reads and caches the full body.
- Keep `proxy.enabled = false` when you want proxying fully disabled; the crate will not inherit environment proxies.
- Prefer passing a scoped `prefix_view("http")` to `from_config`/`create_from_config`, so error paths preserve useful context.
