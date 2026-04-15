# qubit-http User Guide

This guide is based on the current source code and tests. It applies to crate `qubit-http` 0.3.1, imported from Rust code as `qubit_http`.

`qubit-http` is an asynchronous HTTP client infrastructure crate. It wraps `reqwest` and provides unified client options, request building, response reading, error classification, TRACE logging with sensitive-header masking, retries, proxies, IPv4-only resolution, request/response interceptors, and Server-Sent Events (SSE) decoding and reconnection.

## Installation And Imports

```toml
[dependencies]
qubit-http = "0.3.1"
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

    let client = HttpClientFactory::new().create_with_options(options)?;
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
let client = qubit_http::HttpClientFactory::new().create()?;
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
use qubit_http::{Delay, HttpClientFactory, HttpClientOptions, HttpRetryMethodPolicy};

let mut options = HttpClientOptions::new();
options.set_base_url("https://api.example.com")?;
options.user_agent = Some("my-service/1.0".to_string());
options.max_redirects = Some(5);
options.timeouts.connect_timeout = Duration::from_secs(3);
options.timeouts.request_timeout = Some(Duration::from_secs(30));
options.retry.enabled = true;
options.retry.max_attempts = 4;
options.retry.delay_strategy = Delay::Exponential {
    initial: Duration::from_millis(100),
    max: Duration::from_secs(2),
    multiplier: 2.0,
};
options.retry.method_policy = HttpRetryMethodPolicy::IdempotentOnly;

let client = HttpClientFactory::new().create_with_options(options)?;
```

`create_with_options` validates options before building the client. Validation includes: all timeout values must be greater than zero; enabled proxies require a non-empty host and non-zero port; a proxy password requires a username; `logging.body_size_limit` must be greater than zero when request or response body logging is enabled; `user_agent` must be non-empty and a valid header value; SSE line and frame limits must be greater than zero.

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

Supported configuration keys:

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
| `sse.max_line_bytes` | SSE single-line byte limit |
| `sse.max_frame_bytes` | SSE single-frame byte limit |

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
    .timeout(Duration::from_secs(10))
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
| `timeout` | Overrides whole-request timeout |
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

`HttpResponse` exposes `meta()`, `status()`, `headers()`, `url()`, `request_url()`, `is_success()`, `retry_after_hint()`, and body-reading helpers.

```rust
let mut response = client.execute(request).await?;
let text = response.text().await?;
```

Body APIs:

| Method | Behavior |
| --- | --- |
| `bytes_body()` | Lazily reads full body bytes and caches them for later calls |
| `text()` | Decodes the full body as UTF-8 |
| `json<T>()` | Deserializes the full body as JSON |
| `stream_body()` | Returns `HttpByteStream`; consumes the backend stream if the body is not already cached |
| `buffered_body()` | Checks whether the full body is already cached |
| `into_error_body_preview(max_bytes)` | Consumes the response and renders a bounded error body preview |

The backend response body can only be consumed once. Calling `bytes_body`, `text`, or `json` caches the complete body; after that, `stream_body` returns a one-chunk stream from the cache. If you call `stream_body` first and the body was not cached, later full-body reads no longer have the original backend response to read from.

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

```rust
InvalidUrl, BuildClient, ProxyConfig,
ConnectTimeout, ReadTimeout, WriteTimeout, RequestTimeout,
Transport, Status, Decode, SseProtocol, SseDecode,
Cancelled, Other
```

`retry_hint()` marks timeouts, transport errors, 429, and 5xx statuses as retryable hints. Actual retry behavior still depends on `HttpRetryOptions` and the method policy.

## Automatic Retry

Automatic retry is disabled by default. When enabled, `execute` enters the retry flow only when `retry.enabled = true`, `max_attempts > 1`, and the method policy allows the current HTTP method.

Default retryability:

| Type | Default retryable values |
| --- | --- |
| Status | 429 and all 5xx |
| Non-status errors | connect/read/write/request timeout and transport |
| Methods | GET, HEAD, PUT, DELETE, OPTIONS, TRACE |

You can configure `retry.status_codes` and `retry.error_kinds` allowlists. Once an allowlist is set, only listed statuses or error kinds are retried.

Per-request override example:

```rust
let request = client
    .request(Method::POST, "/submit")
    .force_retry()
    .retry_method_policy(qubit_http::HttpRetryMethodPolicy::AllMethods)
    .honor_retry_after(true)
    .build();
```

`honor_retry_after(true)` is request-level. For retryable 429 or 5xx responses, if `Retry-After` is present, the retry controller waits at least that duration before the next attempt.

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

SSE event decoding starts from `HttpResponse`:

```rust
use futures_util::StreamExt;

let response = client
    .execute(client.request(Method::GET, "/stream").build())
    .await?;

let mut events = response.decode_sse_events();
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
- Line and frame limits come from client options, or can be overridden with `decode_sse_events_with_limits`.

### SSE JSON Chunks

```rust
use futures_util::StreamExt;
use qubit_http::sse::{DoneMarkerPolicy, SseChunk};

let response = client.execute(request).await?;
let mut chunks = response.decode_sse_json_chunks::<MyChunk>(DoneMarkerPolicy::DefaultDone);

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

`SseJsonMode::Lenient` skips malformed JSON chunks and continues. `Strict` returns `HttpErrorKind::SseDecode` on the first malformed JSON chunk. Set the default mode on `HttpClientOptions`, or override it per response read with `decode_sse_json_chunks_with_mode` or `decode_sse_json_chunks_with_mode_and_limits`.

### SSE Auto-reconnect

```rust
use futures_util::StreamExt;
use qubit_http::sse::SseReconnectOptions;

let request = client.request(Method::GET, "/events").build();
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
    println!("{}", event.data);
}
```

The default reconnect settings are 3 reconnects, 1 second base delay, reconnect on EOF, and honor server `retry:`. Reconnects reuse the original request. If a previous SSE event had an `id:`, the next request includes `Last-Event-ID`. Cancellation does not reconnect. SSE protocol errors do not reconnect by default. Retryable timeout, transport, 429/5xx, and unexpected-EOF-like errors may reconnect.

## Practical Advice

- Enable global retry for read-only or idempotent APIs. Use `AllMethods` or per-request force retry for POST/PATCH only when the operation is safe to replay.
- Set a realistic `read_timeout` for long-lived streams/SSE; too short a value turns a slow but healthy stream into `ReadTimeout`.
- Disable `logging.log_response_body` when you need true streaming consumption, because TRACE response-body logging pre-reads and caches the full body.
- Keep `proxy.enabled = false` when you want proxying fully disabled; the crate will not inherit environment proxies.
- Prefer passing a scoped `prefix_view("http")` to `from_config`/`create_from_config`, so error paths preserve useful context.
