# qubit-http User Guide

[中文](user_guide.zh_CN.md) | [README](../README.md) | [API Reference](https://docs.rs/qubit-http)

This guide is based on the current source code and tests. It applies to crate `qubit-http` 0.16.0, imported from Rust code as `qubit_http`.

`qubit-http` is an asynchronous HTTP client infrastructure crate. It wraps `reqwest` and provides unified client options, request building, response reading, error classification, TRACE logging with URL/header/body redaction, retries, proxies, IPv4-only resolution, request/response interceptors, and Server-Sent Events (SSE) decoding and reconnection.

## Purpose And Audience

This guide is for Rust service authors who need several API clients to share the
same request, timeout, retry, logging, and response-handling rules. It covers
`qubit-http` 0.16.0 and the public `qubit_http` API. It does not replace the
upstream service's API contract, and it does not make unsafe request replay or
application-level authentication decisions on the caller's behalf.

## How To Read This Guide

| Goal | Start with |
| --- | --- |
| First integration | “Quick Start”, “Building Requests”, “Reading Responses” |
| Client configuration | “Creating A Client”, “Loading From qubit-config”, “Configuration Reference” |
| Failure diagnosis | “Error Model”, “Automatic Retry”, “Logging Redaction” |
| Streaming or SSE | “Reading Responses”, “SSE Decoding” |

## Conceptual Model

An `HttpClient` owns shared execution policy. An `HttpRequest` combines a method, URL, headers, body, and request-level overrides. `execute` applies the client policy and returns an `HttpResponse` only for a 2xx status; the response metadata can be inspected before choosing a body-reading strategy; buffered responses remain reusable, while an unbuffered backend stream can be transferred only once. Retryable failures before a response is returned stay inside the retry boundary, while errors encountered later while reading a stream belong to the caller.

## Installation And Imports

<!-- guide-dependencies:start -->
```toml
[dependencies]
qubit-http = "0.16.0"
qubit-redact = "0.10"
http = "1.4"
qubit-config = { version = "0.15", default-features = false }
qubit-retry = "0.26"
qubit-budget = { version = "0.10", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "sync"] }
futures-util = "0.3"
bytes = "1.11"
```
<!-- guide-dependencies:end -->

```rust
use http::Method;
use qubit_http::{HttpClient, HttpClientOptions};
```

## Quick Start

This complete client program calls an HTTP API, attaches a client identifier,
and reads the response body. Before running it, start a test service that
accepts `GET /anything` and returns a UTF-8 response body. Set
`HTTP_EXAMPLE_BASE_URL` to that service's base URL, then run
`HTTP_EXAMPLE_BASE_URL=http://127.0.0.1:8080 cargo run`. This command does not
start the service.

<!-- guide-check:quick-start:start -->
```rust
use http::Method;
use qubit_http::HttpClient;
use qubit_http::HttpClientOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base_url = std::env::var("HTTP_EXAMPLE_BASE_URL")?;
    let mut options = HttpClientOptions::new();
    options.set_base_url(&base_url)?;
    options.add_header("x-client-id", "demo")?;
    let client = HttpClient::builder().options(options).build()?;
    let request = client
        .request(Method::GET, "/anything")
        .query_param("from", "local-quick-start")
        .build();
    let mut response = client.execute(request).await?;
    println!("status = {}", response.status());
    println!("text = {}", response.text().await?);
    Ok(())
}
```
<!-- guide-check:quick-start:end -->

The program succeeds only for a 2xx response and prints that response's status
and body, which depend on the service. A missing environment variable,
connection failure, non-2xx response, or body decoding failure returns an
error. For a self-contained local run, use
`cargo run --example local_quick_start`; it starts a local server, checks the
response, and prints `status = 200 OK` and `text = ready`. See
[`examples/local_quick_start.rs`](../examples/local_quick_start.rs) for its
full source; it needs no external service or credentials.

`execute` treats only 2xx status codes as success. Non-2xx responses return `HttpErrorKind::Status` with status, method, URL, a bounded response-body preview, and a parsed `Retry-After` value when available.

Use `HttpClient::builder().build()` for defaults, `HttpClient::builder().options(options).build()` for explicit options, and `HttpClient::from_config(config)` for configuration. The older `HttpClientBuilder::create`, `create_default`, and `create_from_config` methods remain available but are deprecated. In particular, `.options(saved).create_default()` ignores `saved`: use `.options(saved).build()` when those options should apply, or a fresh `HttpClient::builder().build()` when defaults are intended.

## Creating A Client

### Default Client

```rust
let client = qubit_http::HttpClient::builder().build()?;
```

Default behavior:

| Item | Default |
| --- | --- |
| `base_url` | None; use an explicit absolute URL, or set a request/client base URL for relative paths |
| `origin_policy` | `SameOrigin` by default. Initial URL checks use the request snapshot; redirect checks use the executing client backend. See [origin policy across clients](#origin-policy-across-clients). |
| Connect timeout | 10 seconds |
| Read timeout | 120 seconds |
| Response-header/send-phase timeout | 120 seconds |
| Reqwest per-attempt network request and response-body deadline | None |
| Proxy | Explicit proxy disabled, and `use_env_proxy = false`; calls reqwest `no_proxy()`, so environment proxies are not inherited |
| Logging | Enabled, but logs are emitted only when tracing TRACE is active |
| Log body preview | 16 KiB |
| Non-success response preview | 16 KiB |
| Whole-response aggregation (`response_body_size_limit`) | 8 MiB |
| Raw non-success body capture (`error_response_body_limit`) | 64 KiB |
| Automatic retry | Disabled |
| Retry max attempts | 3, including the first attempt |
| Retry method policy | Idempotent methods only |
| Log redaction | Built-in common auth/token/cookie/secret/password names for headers, query parameters, and body fields |
| IPv4-only | Disabled |
| SSE JSON mode | `Lenient` |
| SSE done-marker policy | `DefaultDone`, recognizing `[DONE]` |
| SSE line limit | 64 KiB |
| SSE frame limit | 1 MiB |

### Origin policy across clients

A request captures the originating client's base URL and origin policy when it
is built. The client passed to `execute` supplies the network backend, including
its redirect policy:

| Stage | Policy owner |
| --- | --- |
| Initial URL resolution and origin check | Request-building client snapshot |
| Redirect origin check | Executing client's backend |

For example, client A can build a `SameOrigin` request and client B with
`AnyOrigin` can execute it. The initial URL is still checked against A's
snapshot, while redirects follow B's policy. If B uses `SameOrigin`, its
redirect policy restricts redirects even when A used `AnyOrigin`. Rebuilding a
client does not rebind existing requests. Build a new request with the desired
client configuration when both stages should use that client's policy.

`AnyOrigin` allows cross-origin initial targets when selected by the request
snapshot and cross-origin redirects when selected by the executing backend.
Log redaction settings and `HeaderValue::set_sensitive(true)` affect diagnostic
rendering; they do not filter headers sent over the network. A custom API-key
header may therefore be forwarded on an allowed cross-origin redirect, while
reqwest removes its fixed set of standard authentication headers. Keep
`SameOrigin` for requests carrying credentials. If a cross-origin request is
required, use a separate client/request without those credentials and validate
the destination in application code.

### Programmatic Options

```rust
use std::time::Duration;
use qubit_http::{HttpClient, HttpClientOptions, HttpRetryMethodPolicy};
use qubit_retry::BackoffPolicy;

let mut options = HttpClientOptions::new();
options.set_base_url("https://api.example.com")?;
options.user_agent = Some("my-service/1.0".to_string());
options.max_redirects = Some(5);
options.timeouts.connect_timeout = Duration::from_secs(3);
options.timeouts.request_timeout = Some(Duration::from_secs(30));
options.retry.enabled = true;
options.retry.max_attempts = 4;
options.retry.backoff = BackoffPolicy::exponential(
    Duration::from_millis(100),
    2.0,
    Duration::from_secs(2),
)?;
options.retry.method_policy = HttpRetryMethodPolicy::IdempotentOnly;

let client = HttpClient::builder().options(options).build()?;
```

`create` validates options before building the client. Validation includes: all timeout values must be greater than zero; `response_header_timeout` must also fit in the platform's `Instant` deadline range; enabled proxies require a non-empty host and non-zero port; a proxy password requires a username; `logging.body_size_limit` must be greater than zero when request or response body logging is enabled; `retry.max_attempts` must be greater than zero; `error_response_preview_limit` must be greater than zero; `user_agent` must be non-empty and a valid header value; SSE line and frame limits must be greater than zero. Backoff and jitter values are validated when constructing `BackoffPolicy`.

When changing options after registering header injectors or interceptors, rebuild through the existing client:

```rust
let mut updated = client.options().clone();
updated.timeouts.request_timeout = Some(std::time::Duration::from_secs(20));
let rebuilt = client.rebuild_with_options(updated)?;
```

`rebuild_with_options` creates a new backend connection pool and retains all four kinds of hook registration in their original order; the existing client remains unchanged if building fails. `client.to_builder()` copies only options. Finish that flow with `.build()` to use the copied options; runtime hooks are not copied.

```rust
let rebuilt_from_copied_options = client.to_builder().build()?;
```

### Additional TLS Trust Roots

`HttpClientOptions::extra_root_certificates_der` lets the caller add DER-encoded
certificates to the trust roots used by the selected TLS backend. Each
`Bytes` value must contain exactly one DER certificate. The list is additive:
it preserves the backend's default trust sources and does not replace them.
An empty list adds nothing. This is programmatic configuration; there is no
new `qubit-config` key and the library does not read certificate files
automatically. The caller owns file loading and I/O error handling:

<!-- guide-check:tls-roots:start -->
```rust
let mut options = qubit_http::HttpClientOptions::new();
let certificate_der = std::fs::read("service-ca.der")?;
options.extra_root_certificates_der.push(bytes::Bytes::from(certificate_der));
let client = qubit_http::HttpClient::builder().options(options).build()?;
let _ = client;
```
<!-- guide-check:tls-roots:end -->

The selected TLS backend supplies the default trust roots (for example, its
platform or bundled store); the additional certificates are merged with that
default. Invalid DER is reported while `build()` constructs the client. If the
selected TLS backend or platform cannot use an added root, client construction
fails instead of silently ignoring it. Rebuilding from options that still
contain the certificates keeps them; clear the list before rebuilding to
return to the backend defaults:

```rust
let mut updated = client.options().clone();
updated.extra_root_certificates_der.clear();
let rebuilt = client.rebuild_with_options(updated)?;
```

### Loading From qubit-config

`HttpClientOptions::from_config` and `HttpClient::from_config` accept any `qubit_config::ConfigReader`. If you pass `config.section("http")`, all keys below are read relative to that prefix.

<!-- guide-check:configuration:start -->
```rust
use std::time::Duration;
use qubit_config::Config;
use qubit_http::HttpClient;

let mut config = Config::new();
config.set("http.base_url", "https://api.example.com".to_string())?;
config.set("http.timeouts.connect_timeout", Duration::from_secs(3))?;
config.set("http.retry.enabled", true)?;
config.set("http.retry.delay_strategy", "FIXED".to_string())?;
config.set("http.retry.fixed_delay", Duration::from_millis(250))?;

let client = HttpClient::from_config(&config.section("http")?)?;
```
<!-- guide-check:configuration:end -->

`HttpClient::from_config` is the recommended configuration entry point. The deprecated `create_from_config` remains available for compatibility and ignores options previously stored on its builder.

`from_config` rejects unknown fields in the selected component scope. For
example, `http.timeouts.connect_timout` fails instead of silently using the
default timeout. The independent `HttpTimeoutOptions`, `ProxyOptions`,
`HttpLoggingOptions`, and `HttpRetryOptions` constructors enforce their own
schemas and return existing domain validation failures during construction.
Both conversion and domain errors report root-relative paths, such as
`service.http.timeouts.connect_timeout`.

`default_headers.*` remains an open map accepting custom header names. Select
`config.section("http")?` when using a shared configuration; sibling sections
are not checked. Put interpolation helpers such as `${shared.host}` in root
`shared.*` keys outside the component scope; root fallback still resolves them.
Previously ignored legacy fields, such as root `sensitive_headers`, now fail
and must be migrated to declared fields. Conversion failures from rs-config
sources retain their final loading origin through `Error::source()`.

Common configuration keys:

| Key | Description |
| --- | --- |
| `base_url` | Base URL used to resolve relative request paths |
| `origin_policy` | `same_origin` (default) checks initial URLs using the request snapshot; redirects use the executing client backend policy. `any_origin` permits cross-origin URLs at the stage controlled by that policy; see [origin policy across clients](#origin-policy-across-clients) |
| `timeouts.connect_timeout` | Connect timeout |
| `timeouts.read_timeout` | Bounds each normal response-body read and the complete non-success error-body capture phase |
| `timeouts.response_header_timeout` | Cumulative asynchronous preparation and send deadline through response headers; synchronous request interceptors run before this deadline starts |
| `timeouts.request_timeout` | Optional reqwest per-attempt network request and response-body deadline; excludes async header injection, streaming-body factory preparation, retry waits, and other attempts |
| `proxy.enabled` | Enables outbound proxying |
| `use_env_proxy` | Whether to inherit environment proxies when explicit proxying is disabled |
| `logging.enabled` | Allows TRACE HTTP logs |
| `json.max_depth` | Maximum nesting depth for response, SSE, and configured-header JSON |
| `json.max_nodes` | Maximum total nodes in one JSON value |
| `json.max_output_bytes` | Maximum aggregate encoded bytes for JSON and NDJSON request bodies |
| `log_redaction.url_path_policy` | URL path policy: `preserve` by default, or explicit `redact` |
| `log_redaction.sensitive_headers` | Extra sensitive header names added to the default log-redaction set |
| `log_redaction.sensitive_query_params` | Extra sensitive query-parameter names added to the default set |
| `log_redaction.sensitive_body_fields` | Extra sensitive JSON/form/multipart body-field names added to the default set |
| `log_redaction.excluded_sensitive_headers` | Exact header names explicitly allowed to remain visible |
| `log_redaction.excluded_sensitive_query_params` | Exact query-parameter names explicitly allowed to remain visible |
| `log_redaction.excluded_sensitive_body_fields` | Exact structured-body field names explicitly allowed to remain visible |
| `retry.enabled` | Enables built-in retry |
| `retry.max_attempts` | Max attempts, including the first request |
| `retry.delay_strategy` | `NONE`, `FIXED`, `RANDOM`, `EXPONENTIAL_BACKOFF`, or `EXPONENTIAL` |
| `retry.method_policy` | `IDEMPOTENT_ONLY`/`IDEMPOTENT`, `ALL_METHODS`/`ALL`, or `NONE`/`DISABLED` |
| `retry.status_codes` | Retryable status allowlist; defaults to 429 and 5xx when absent |
| `retry.error_kinds` | Retryable non-status error-kind allowlist; defaults to timeouts and transport when absent |
| `sse.json_mode` | `LENIENT` or `STRICT` |
| `sse.done_marker` | `DISABLED` or `DISABLE` disables done markers; `DEFAULT` uses `[DONE]`; any other non-empty string becomes a `Custom` marker compared to trimmed `data:` text |
| `sse.max_line_bytes` | SSE single-line byte limit |
| `sse.max_frame_bytes` | Maximum cumulative UTF-8 bytes of non-empty, non-comment logical lines between blank-line boundaries; unknown fields count, terminators and comments do not. Comment lines remain subject to `sse.max_line_bytes`. This is not a wire-size or whole-session limit. |

See “Configuration Reference” at the end of this guide for the full key list.

`default_headers` supports two forms, but they cannot be used at the same time. The subkey form is:

```rust
config.set("http.default_headers.authorization", "Bearer token".to_string())?;
config.set("http.default_headers.x-request-id", "abc-123".to_string())?;
```

`default_headers` can also be a JSON map string; parsing fails if the JSON map and `default_headers.*` subkeys are both configured. Header names are case-insensitive, so two configured names that differ only by case (for example, `Authorization` and `authorization`) are rejected in either form instead of overwriting one another:

```rust
config.set(
    "http.default_headers",
    r#"{"x-app-id":"demo","x-version":"1.0"}"#.to_string(),
)?;
```

## Building Requests

Use `client.request(method, path)` to create an `HttpRequestBuilder`. `path` can be an absolute URL or a relative path. With the default `SameOrigin` policy, an explicit absolute URL is allowed when no `base_url` is configured and becomes the origin for that request; when `base_url` is configured, the URL must share its scheme, host, and effective port. Relative paths require a `base_url`. `AnyOrigin` must be selected explicitly to allow cross-origin initial targets. Initial URL checks use the policy captured by the client that built the request; redirects use the policy of the client that executes it. See [origin policy across clients](#origin-policy-across-clients).

Request URLs and client/request `base_url` values must use `http` or `https`. Unsupported schemes such as `ftp`, `file`, and `data` are rejected as `InvalidUrl` at request resolution; an unsupported client `base_url` is rejected as a `base_url` configuration error. This prevents unsupported targets from being misclassified as retryable transport failures.

```rust
let request = client
    .request(Method::POST, "/events")
    .query_params([("source", "mobile"), ("debug", "false")])
    .header("x-request-id", "req-001")?
    .json_body(&serde_json::json!({"name": "created"}))?
    .request_timeout(Duration::from_secs(10))?
    .read_timeout(Duration::from_secs(30))?
    .build();
```

After `build()`, `HttpRequest::resolved_url()` returns the actual outbound URL: absolute `path` values stay absolute, relative paths are joined with `base_url`, query pairs already present in `path` are preserved, and builder/query-interceptor pairs are appended in order.

Relative paths follow `Url::join` semantics. A path beginning with `/` starts at the host root, and a base URL without a trailing slash treats its final path segment as a file to replace. To keep an API prefix such as `/v1/`, end the base URL with `/` and use a relative path without a leading slash.

| `base_url` | `path` | resolved URL |
| --- | --- | --- |
| `https://api.example.com/v1/` | `events` | `https://api.example.com/v1/events` |
| `https://api.example.com/v1/` | `/events` | `https://api.example.com/events` |
| `https://api.example.com/v1` | `events` | `https://api.example.com/events` |
| `https://api.example.com/v1` | `/events` | `https://api.example.com/events` |

This complete example checks the resolved URL for each combination without sending a request:

<!-- guide-check:url-join:start -->
```rust
use http::Method;
use qubit_http::{HttpClient, HttpClientOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for (base, path, expected) in [
        ("https://api.example.com/v1/", "events", "https://api.example.com/v1/events"),
        ("https://api.example.com/v1/", "/events", "https://api.example.com/events"),
        ("https://api.example.com/v1", "events", "https://api.example.com/events"),
        ("https://api.example.com/v1", "/events", "https://api.example.com/events"),
    ] {
        let mut options = HttpClientOptions::default();
        options.set_base_url(base)?;
        let client = HttpClient::builder().options(options).build()?;
        assert_eq!(client.request(Method::GET, path).build().resolved_url()?.as_str(), expected);
    }
    Ok(())
}
```
<!-- guide-check:url-join:end -->

```rust
let request = client
    .request(Method::GET, "/users?active=true")
    .query_param("expand", "profile")
    .build();

let url = request.resolved_url()?;
assert_eq!(
    url.as_str(),
    "https://api.example.com/users?active=true&expand=profile",
);
```

Request body builders:

| Method | Behavior |
| --- | --- |
| `bytes_body` | Raw bytes; does not infer a media type and removes a previous builder-generated `Content-Type` |
| `stream_body` | Eagerly collects a finite iterator into memory during this call, then sends the ordered chunks through reqwest streaming body support; memory grows with the total payload. Despite the collection, reqwest treats it as streaming and cannot replay it across an internal 307/308 redirect; removes a previous builder-generated `Content-Type` |
| `streaming_body` | Installs an async factory that creates a fresh byte stream for each outer send attempt; it is not called again for an internal 307/308 redirect, which is not followed for streaming bodies; removes a previous builder-generated `Content-Type` |
| `text_body` | Text body; sets or refreshes the builder-generated `text/plain; charset=utf-8` value |
| `json_body` | Serializes JSON; sets or refreshes the builder-generated `application/json` value |
| `json_body_with_limits` | Serializes JSON with per-request value and output-byte limits |
| `form_body` | Form body; sets or refreshes the builder-generated `application/x-www-form-urlencoded` value |
| `multipart_body` | Raw multipart bytes; requires a 1 to 70 character token-safe boundary; refreshes a builder-generated multipart value for the new boundary, adds a missing boundary to an explicit multipart value, and rejects duplicate, malformed, or mismatched boundary parameters |
| `ndjson_body` | One JSON record per line; sets or refreshes the builder-generated `application/x-ndjson` value |
| `ndjson_body_with_limits` | Serializes an NDJSON sequence with one aggregate output budget, including line terminators |

Typed body helpers update the `Content-Type` they previously generated when you replace one typed body with another. `bytes_body`, `stream_body`, and `streaming_body` remove a builder-generated value because they do not infer a media type. A request-local value supplied with `header` or `headers` is explicit—even when it matches a generated value—and body helpers preserve it. You can set that value before or after a body helper. When an opaque body removes a generated request-local value, the normal merge can expose a client default or injector value again; those client-level sources are not cleared.

If an existing chain relied on an earlier helper's generated value surviving a later body replacement, make that choice explicit:

```rust
let request = client.request(Method::POST, "/items")
    .header("content-type", "text/plain; charset=utf-8")?
    .text_body("old")
    .json_body(&serde_json::json!({"ready": true}))?
    .build();
assert_eq!(request.headers().get("content-type").unwrap(), "text/plain; charset=utf-8");
```

When upgrading to a build with this behavior change, review chains that call more than one body helper. Add an explicit `header("content-type", ...)` where the earlier media type is intentional; otherwise, the final typed helper supplies its own type, and an opaque helper leaves the request-local type unset.

`HttpRequest::set_body` and `set_streaming_body` are lower-level replacements and preserve headers without inferring a media type. Pair either setter with `set_header` or `remove_header` when the media type must change.

Per-request overrides:

| Method | Purpose |
| --- | --- |
| `request_timeout` | Optional reqwest per-attempt network request and response-body deadline; excludes async header injection, streaming-body factory preparation, retry waits, and other attempts |
| `response_header_timeout` | Overrides the cumulative asynchronous preparation and send deadline through response headers; synchronous request interceptors run before this deadline starts |
| `read_timeout` | Bounds each normal body read and complete non-success error-body capture; expiry retains status and incomplete partial raw body |
| `base_url` / `clear_base_url` | Overrides or removes base URL for this request |
| `ipv4_only` | Overrides IPv4-only URL validation for this request |
| `cancellation_token` | Binds a `CancellationToken`, checked before send, during send, and during body/stream reads |
| `force_retry` | Force-enables retry for this request |
| `disable_retry` | Disables retry for this request |
| `retry_method_policy` | Overrides retryable HTTP method policy for this request |
| `honor_retry_after` | Honors `Retry-After` for retryable 429/5xx responses on this request |

`request_timeout` does not cover async header injection or streaming-body factory preparation. A request initially snapshots this value from its creating client; `None` uses the executing client's backend default, and `clear_request_timeout()` restores that behavior. `response_header_timeout` starts after synchronous request interceptors and covers asynchronous preparation through response headers. `read_timeout` bounds each normal response-body read and the complete capture of a non-success error body; on expiry, the returned `Status` error retains its status and partial raw body marked incomplete. Inspect `is_truncated()` and increase `read_timeout` when slow error bodies must be retained. Per-request timeout overrides reject zero durations and return `HttpError`.

## Headers, Injectors, And Interceptors

Final request headers are merged in this order; later steps override duplicate names:

1. Client default headers snapshotted when the builder is created.
2. Synchronous `HttpHeaderInjector`s in registration order.
3. Asynchronous `AsyncHttpHeaderInjector`s in registration order.
4. Request-local headers.

<!-- guide-check:async-upload:start -->
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

let upload = client
    .request(http::Method::POST, "https://api.example.com/upload")
    .streaming_body(|| {
        Box::pin(async move {
            Box::pin(futures_util::stream::iter(vec![Ok(bytes::Bytes::from_static(b"chunk"))]))
                as qubit_http::HttpRequestBodyByteStream
        })
    })
    .build();
let _ = upload;
```
<!-- guide-check:async-upload:end -->

Async injectors can resolve headers immediately before sending, including authentication tokens. This complete example reuses a fresh cached token and refreshes it when it expires. When automatic retry is enabled, the injector runs before each send attempt. Concurrent cache misses wait on a refresh lock and check the cache again before refreshing. A failed refresh is not cached, so a later call can retry. The one-hour lifetime is illustrative; use the expiry returned by the auth service. Cancellation releases the local lock, but cannot undo a refresh already performed by the service, so this does not promise exactly-once remote effects.

<!-- guide-check:token-refresh:start -->
```rust
use std::sync::Arc;
use std::time::{Duration, Instant};

use http::header::{AUTHORIZATION, HeaderValue};
use qubit_http::{AsyncHttpHeaderInjector, HttpClient, HttpError, HttpResult};
use tokio::sync::{Mutex, RwLock};

struct CachedToken {
    value: String,
    expires_at: Instant,
}

struct TokenProvider {
    cached: RwLock<Option<CachedToken>>,
    refresh_lock: Mutex<()>,
}

impl TokenProvider {
    fn new() -> Self {
        Self {
            cached: RwLock::new(None),
            refresh_lock: Mutex::new(()),
        }
    }

    async fn bearer_token(&self) -> HttpResult<String> {
        {
            let cached = self.cached.read().await;
            if let Some(token) = cached.as_ref() {
                if token.expires_at > Instant::now() + Duration::from_secs(30) {
                    return Ok(token.value.clone());
                }
            }
        }
        let _refresh_guard = self.refresh_lock.lock().await;
        {
            let cached = self.cached.read().await;
            if let Some(token) = cached.as_ref() {
                if token.expires_at > Instant::now() + Duration::from_secs(30) {
                    return Ok(token.value.clone());
                }
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = HttpClient::builder().build()?;
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
    Ok(())
}
```
<!-- guide-check:token-refresh:end -->

Request interceptors run before each send attempt. They can mutate `HttpRequest`; returning an error short-circuits execution. Response interceptors run only for successful-status responses. They receive `HttpResponseInterceptorContext`: status and request method are immutable, while response headers and the final response URL can be mutated. Returning an error makes `execute` fail.

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

client.add_response_interceptor(HttpResponseInterceptor::new(|context| {
    if !context.headers().contains_key("x-required") {
        return Err(HttpError::other("missing x-required response header"));
    }
    Ok(())
}));
```

## Reading Responses

`HttpResponse` exposes `meta()`, `status()`, `headers()`, `url()`, `request_url()`, `is_success()`, `retry_after_hint()`, and helpers to read the body or decode SSE. After `let mut response = client.execute(...).await?`, choose a body-reading strategy from the table below. An unbuffered backend body can be transferred to a stream once; once buffered, it remains available for repeated reads and streams. The **Examples** subsection right after the table shows typical usage for each API listed there.

Body APIs:

| Method | Behavior |
| --- | --- |
| `bytes()` | Lazily reads the full body on the first `await`, caches it, then reuses the cache |
| `text()` | Full-body UTF-8 decode via `bytes()` |
| `json<T>()` | Full-body JSON via `bytes()` |
| `stream()` | Returns `HttpByteStream`; if the body is cached, yields a one-chunk in-memory stream even after request cancellation. Otherwise the call does **not** eagerly read the entire body: it hands the backend response to the returned stream, and bytes are read **while that stream is polled** |
| `sse_messages()` | Consumes `self` and decodes the body stream into an SSE message stream using the configured line/frame limits (see “SSE Decoding” below) |
| `sse_chunks::<T>()` | Consumes `self` and decodes SSE JSON `data:` chunks into a `SseChunk<T>` stream (JSON mode, done-marker policy, etc.—see “SSE JSON Chunks” below) |

`bytes()`, `text()`, and `json()` aggregate the whole response and share the
`response_body_size_limit` default of 8 MiB. Exceeding it returns
`HttpErrorKind::ResponseBodyTooLarge`; a JSON helper is also subject to the
independent JSON value and payload limits. For a trusted endpoint whose
expected response size is known, set a suitable aggregate budget when building
the client. For a large or unpredictable download, choose `stream()` before
reading the body and process chunks incrementally. Streaming avoids this
whole-body aggregation limit, but the application should still bound its own
accumulated bytes, storage, and processing work.

### Examples

**Whole-body reads (`bytes` / `text` / `json`)**: each loads the entire body into the cache. Cached bytes can be reused by whole-body readers and `stream()`; a successful cache read does not prevent creating a stream. This example uses `response` from the existing `client.execute(...).await?` in the same async function:

```rust
use futures_util::StreamExt;
use http::Method;
use serde::Deserialize;

#[derive(Deserialize)]
struct VersionInfo {
    version: String,
}

async fn read_whole_body_examples(
    client: &qubit_http::HttpClient,
) -> qubit_http::HttpResult<()> {
    let mut response = client
        .execute(client.request(Method::GET, "/bytes").build())
        .await?;
    let expected = response.bytes().await?;
    let mut cached_stream = response.stream()?;
    let chunk = cached_stream.next().await.transpose()?;
    assert_eq!(chunk.as_ref(), Some(&expected));
    assert!(cached_stream.next().await.is_none());
    assert_eq!(response.bytes().await?, expected);

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

To raise the aggregate budget for known large responses, configure the client
before sending the request. This example uses a 16 MiB application budget; it
does not change the library default. JSON value and payload limits remain
independent and may still reject a JSON body:

```rust
use qubit_http::{HttpClient, HttpClientOptions};

fn client_for_known_large_responses() -> qubit_http::HttpResult<HttpClient> {
    let options = HttpClientOptions {
        response_body_size_limit: 16 * 1024 * 1024,
        ..HttpClientOptions::default()
    };
    HttpClient::builder().options(options).build()
}
```

**Streaming bytes (`stream`)**: for an unbuffered body, `stream()` transfers the backend body to the returned stream; it cannot be recovered for later whole-body reads. `?` applies when obtaining the stream, for example if a request cancellation token has already fired before body reading starts. A cancellation or body-read failure is retained and cannot be bypassed by another reader. Once a whole-body read succeeds, later cancellation does not block reading the cache or decoding cached SSE; cancellation still stops reads that need backend I/O. Callers that must stop business processing immediately when a token is cancelled should check that token at the application processing boundary. This separate request example shows unbuffered network streaming, rather than cached in-memory bytes.

For `bytes`, `text`, and `json`, an unbuffered backend body is also transferred when the aggregate-read future is first polled. If an outer `timeout` or a losing `select!` branch then drops that future before buffering succeeds, the backend and any partial bytes are lost; later readers return `ResponseBodyAlreadyConsumed`. Dropping an unpolled future does not take the body, and a successfully cached body remains reusable. This external drop differs from cancellation or read-timeout errors returned by the built-in reader, which retain their existing failure category. Configure the request read timeout or cancellation token before sending when a classified read failure is needed; treat an externally cancelled unbuffered response as unreadable afterward.

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

Process each chunk and release or store it according to application policy;
collecting every chunk into one unbounded buffer would recreate the memory
risk of whole-body aggregation. If an aggregate read returns
`ResponseBodyTooLarge`, that response body has already failed its read and
cannot be recovered by switching the same response to `stream()`. If the
operation needs another request, the application must decide whether replay is
safe; the client does not automatically retry body-read failures.

**SSE (`sse_messages` / `sse_chunks`)**: both **consume** `self` (move the response). To tweak line/frame limits, JSON mode, or done-marker policy before decoding, chain the `sse_*` configuration methods with `sse_messages()` or `sse_chunks::<T>()` on the same expression (full details under “SSE Decoding” and “SSE JSON Chunks” below).

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
    let mut events = ev.sse_messages();
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

After a successful `bytes`, `text`, or `json` read, the cache supports repeated reads and new one-chunk `stream()` calls, including after request cancellation. An unbuffered backend body transfers to its stream and cannot be recovered for whole-body reads; cancellation or an existing body-read failure remains an error. `sse_messages` / `sse_chunks` also use the stream path and **consume** the `HttpResponse` by value; cached SSE bodies remain decodable after cancellation.

| Safe | Avoid |
| --- | --- |
| Use `bytes()` / `text()` / `json()` to read and reuse the cached full body | Call `stream()` first, then call `bytes()` / `text()` / `json()` on the same response |
| Call `stream()` after `bytes()`; the cached response remains available for later reads or streams | Call `sse_messages()` or `sse_chunks()` and then try to use the moved response value |
| Chain `sse_*` option setters with the SSE consumer on the same expression | Design multiple body-consumption paths for one `HttpResponse` |

`retry_after_hint()` returns a delay for metadata with status 429 or 5xx and a valid `Retry-After` header. It supports `delta-seconds` and HTTP-date formats; past dates resolve to 0 seconds. `execute` maps non-success statuses to `HttpError` before success response interceptors run, so obtain a 429/5xx hint from the error:

```rust,ignore
match client.execute(request).await {
    Ok(response) => handle_response(response).await?,
    Err(error) => {
        if let Some(delay) = error.retry_after {
            schedule_retry(delay);
        } else {
            return Err(error.into());
        }
    }
}
```

`HttpResponseMeta` and `HttpResponseInterceptorContext` retain `retry_after_hint()` for metadata inspected outside the success-only interceptor path and for manually constructed metadata.

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

For non-2xx errors, the redacted preview and retained raw body have independent limits. The preview can be truncated while the raw body remains complete. `HttpStatusResponse::is_truncated()` retains its compatibility meaning: it is true if raw bytes were omitted or capture ended before completeness was established, and it does not identify the cause. Use `capture_end()` to distinguish `Complete`, `SizeLimit`, `ReadTimeout`, `ReadFailure`, and `BodyUnavailable`; the non-exhaustive enum requires a fallback match arm. `capture_error()` returns the backend error and source chain only for `ReadFailure`; redact it before logging. This is independent of the top-level `HttpError::source()` retry chain, and the snapshot retains it through retry projection. Parse provider fields only when `capture_end()` is `Complete`; preview truncation alone does not make the raw body incomplete. The default raw capture is 64 KiB. Raw bytes may contain sensitive data: do not log them directly, and check capture completeness before parsing.

| Group | Error kinds | Typical meaning |
| --- | --- | --- |
| URL / configuration | `InvalidUrl`, `BuildClient`, `ProxyConfig` | URL resolution failed, client construction failed, or proxy options are invalid |
| Timeout / network | `ConnectTimeout`, `ReadTimeout`, `ResponseHeaderTimeout`, `RequestTimeout`, `Transport` | `ConnectTimeout` is a timeout reported for the connection phase; other send-phase timeouts remain `RequestTimeout`, and non-timeout connection failures remain `Transport` |
| HTTP status | `Status` | A non-2xx status code was returned |
| Decoding / SSE | `Decode`, `SseProtocol`, `SseDecode` | Body decoding failed, SSE framing was invalid, or an SSE JSON chunk could not be decoded |
| Retry layer | `RetryBudgetExceeded` and `HttpRetryDiagnostics` | Retry budget or terminal-policy diagnostics |
| Response body | `ResponseBodyTooLarge`, `ResponseBodyAlreadyConsumed` | Whole-body aggregation exceeded its configured limit, or a body stream was already taken |
| Cancellation / fallback | `Cancelled`, `Other` | The request was cancelled, or the failure does not fit another category |

Ordinary HTTP retry and SSE reconnect have separate terminal-error contracts:

| Path | Terminal condition | Result and direct source |
| --- | --- | --- |
| Ordinary HTTP retry | A final business error exists when attempts or duration are exhausted | Preserve its HTTP kind, attach `HttpRetryDiagnostics`, and retain the complete `RetryError<HttpError>` as source |
| Ordinary HTTP retry | Budget expires before a business error is captured | May return `RetryBudgetExceeded` with the retry error as source; cancellation and other terminal mappings keep their own behavior |
| SSE reconnect | Attempts are exhausted with a retryable error | Return the last HTTP/SSE error with its original source; ordinary HTTP retry diagnostics are not promised |
| SSE reconnect | Operation/total time budget expires | Return `RetryBudgetExceeded`, preserve available request context, and use the last `HttpError` as direct source when one exists |
| SSE reconnect | Budget accounting itself fails | Keep the existing `Other` kind and `RetryBudgetError` source |
| SSE reconnect | Clean EOF exhausts attempts, or response is 204 | End the stream normally |

SSE does not promise the ordinary HTTP retry source type or diagnostics.

`retry_metadata()` is a deprecated compatibility getter that always returns `None`; it does not indicate whether retry execution occurred. Use `retry_diagnostics()` for terminal details and inspect `Error::source()` for the retained `RetryError<HttpError>` context. SSE decoding errors also carry the response method, original request URL, and status when those fields were not already present; URL rendering continues to use the response redaction policy.

`retry_hint()` marks timeouts, transport errors, 429, and 5xx statuses as retryable hints. The new retry-layer error categories are non-retryable themselves. Actual retry behavior still depends on `HttpRetryOptions` and the method policy.

## Troubleshooting

Start with the error's `kind`, `message`, `status`, `url`, and `source`. The
following symptoms identify the most common configuration or usage mistakes:

| Symptom | Check | Corrective action |
| --- | --- | --- |
| Relative request path fails with `InvalidUrl` | Whether `base_url` is set | Configure a client/request base URL or use an absolute URL. |
| Cross-origin URL or redirect is rejected | `origin_policy` and the configured base URL | Use the default `SameOrigin` for isolation, or explicitly select `AnyOrigin` when the service contract requires it. For requests built by one client and executed by another, see [origin policy across clients](#origin-policy-across-clients). |
| Request never retries | `retry.enabled`, `max_attempts`, method policy, and error allowlists | Enable retry and ensure the method and error/status are safe and allowed to replay. |
| Response body read fails after streaming | Whether an unbuffered backend stream was already taken | Buffered responses remain reusable; an unbuffered backend stream cannot be recovered after it is transferred. |
| Logs are absent or sensitive values are visible | `logging.enabled`, TRACE subscriber level, and redaction policy | Enable both logging conditions and configure `RedactionPolicy` before constructing the client. |
| SSE reconnect rejects the response | `Content-Type` and cancellation state | Reconnect requires `text/event-stream`; cancellation and SSE protocol errors do not reconnect by default. |

For configuration failures, pass a scoped `config.section("http")` to retain
root-relative error paths. For retry failures, inspect `retry_diagnostics()`
and follow `Error::source()` to the retained `RetryError<HttpError>`.
For an incomplete non-2xx body, inspect `status_response().capture_end()` to
distinguish a size bound, capture timeout, backend read failure, and unavailable
stream. `capture_error()` is available for backend read failures; redact its
details before logging. Only parse the retained body as provider JSON when the
capture end is `Complete`.

### SSE reconnect cancellation ownership

The original request cancellation token belongs to the reconnect call: it can stop reconnect waiting and prevent another attempt. A request interceptor may replace it with an independent token for that attempt’s send and response-body reads, or remove the attempt token. Replacing/removing the attempt token does not change the original reconnect-level cancellation source.

## Automatic Retry

Automatic retry is disabled by default. When enabled, `execute` enters the retry flow only when `retry.enabled = true`, `max_attempts > 1`, and the method policy allows the current HTTP method.

`max_attempts` and retry diagnostics count this crate's outer attempts. A single reqwest execution may perform safe protocol recovery and redirects; neither creates another outer attempt. Disabling this crate's retry stops those outer retries but preserves reqwest's safe protocol recovery. Request interceptors and dynamic header injection run per outer attempt, while reqwest recovery does not rerun them. `request_timeout` bounds one reqwest execution and its response-body reads; it is not reset for each lower-level wire send. A request also snapshots its originating client's base URL, origin policy, default headers, injectors, and redactor when built. The executing client supplies the backend, logging options, execution interceptors, and retry policy. If client A builds a request and client B executes it, request/response logs and execution errors use A's redactor; responses and SSE diagnostics keep using that same policy after return. The policies are not merged. Rebuilding a client does not rebind old requests; build a new request with the rebuilt client to use its captured configuration. Initial URL checks use the request snapshot, while redirect checks use the executing backend's redirect policy. Independently constructed `HttpLogger` values continue to use their explicitly supplied policy.

Default retryability:

| Type | Default retryable values |
| --- | --- |
| Status | 429 and all 5xx |
| Non-status errors | connect/read/response-header/request timeout and transport |
| Methods | GET, HEAD, PUT, DELETE, OPTIONS, TRACE |

You can configure `retry.status_codes` and `retry.error_kinds` allowlists. Once an allowlist is set, only listed statuses or error kinds are retried.

`retry.error_kinds` accepts config names for every `HttpErrorKind`. Values are trimmed; hyphenated names such as `read-timeout` are normalized to `read_timeout`. Use lowercase snake_case or the equivalent hyphenated form.

A request interceptor can change the method or retry override after retry flow has been admitted. The final method and override determine whether the flow may continue; an interceptor cannot start a retry flow that was not admitted. See [Headers, Injectors, And Interceptors](#headers-injectors-and-interceptors) for interceptor ordering.

Per-request override example:

```rust
let request = client
    .request(Method::POST, "/submit")
    .force_retry()
    .retry_method_policy(qubit_http::HttpRetryMethodPolicy::AllMethods)
    .honor_retry_after(true)
    .build();
```

`honor_retry_after(true)` is request-level. For retryable 429 or 5xx responses, it passes the response's `Retry-After` value to the backoff policy as a hint. The default HTTP backoff policy uses this hint as a minimum: the next attempt waits for the longer of the planned backoff and the hint. A custom `BackoffPolicy` can change that behavior: `ignore_retry_after()` ignores the hint, and `limit_delay(duration)` caps the final delay even when the hint is longer. SSE reconnect uses the final request preference too: when enabled, a retryable 429/5xx handshake can supply its parsed hint to the reconnect backoff. `server_retry_max_delay` applies only to the SSE `retry:` field; a policy delay cap and the reconnect budget still apply to HTTP hints.

When retry is enabled, `execute` runs attempts through `qubit-retry`'s `Retry`. HTTP `max_duration` uses monotonic accounting. Retryable failures that exhaust `max_attempts` or `max_duration` preserve the last HTTP error and attach `HttpRetryDiagnostics`; the complete `RetryError<HttpError>` remains available as `source`.

This is a **soft continuation budget**: it can reject a planned delay or the next
admission, but it neither cancels an in-flight request nor replaces its successful
result. Ordinary HTTP retry does not set `qubit-retry`'s `attempt_timeout` or
`flow_timeout`. For example, `max_duration = 1s` can still return a successful
request that took 2s. `request_timeout` bounds reqwest network execution and response-body reads for one attempt; it excludes asynchronous header injection, streaming-body factory preparation, retry waits, and other attempts. Use `response_header_timeout` for asynchronous preparation through response headers. `read_timeout` also bounds complete non-success error-body capture; if a slow error body times out, inspect `is_truncated()` and raise `read_timeout` if needed. Bodies returned lazily remain outside the built-in retry boundary.

Retryable requests must be safe to replay. Buffered bodies can be sent again;
`streaming_body` factories must create a fresh stream for each outer attempt, and
header injectors/interceptors may run again. Neither streaming body form is
replayed through reqwest's internal 307/308 redirect handling. When a streaming
upload receives one of those redirects and no outer retry retries that status,
`execute` returns the corresponding `Status` error. Prefer the final upload URL,
or use `bytes_body` when the payload fits the application's memory budget. With
the default HTTP backoff policy,
`honor_retry_after` uses the hint as a minimum; an excessive hint can exhaust the
continuation budget instead of starting another request. Custom hint policies or
final delay caps can change the selected delay. For its independent replay
policy, see [SSE Auto-reconnect](#sse-auto-reconnect).

This release uses `qubit-retry` 0.26. Update any direct dependency and its lockfile
entry when sharing `RetryPolicy` or `BackoffPolicy` with HTTP/SSE. When consuming
retry results directly, `RetryError::map_error` provides pure payload conversion
while preserving retry context and completion diagnostics. HTTP's domain error conversion
still determines the returned `HttpError`; it is not replaced by a generic retry
error API. If you consume retry results directly, inspect
`completion_callback_failures()` or use the lossless `into_parts()` triple.
The old `into_parts_with_diagnostics()` name has been removed. Exhaustive matches on
`RetryCallbackPhase` must include `Success` and `TerminalFailure`.

| Scenario | Returned error | Notes |
| --- | --- | --- |
| Method policy does not allow replay, such as POST under the default policy | Original single-attempt error | Retry flow is not entered |
| Retry flow is active, but the current error is not retryable | Original `HttpError` + `Aborted` diagnostics | `source` retains `RetryError<HttpError>` |
| Retryable failure exhausts `max_attempts` | Last `HttpError` | `message` includes attempts-exhausted context |
| Retryable failure exhausts `max_duration` | Last `HttpError` or `RetryBudgetExceeded` | Returns the last error if one was captured; otherwise returns `RetryBudgetExceeded` |

To inspect retry completion and the underlying retry context, use the supported diagnostics and source chain. `retry_metadata()` remains only for source compatibility; it is deprecated and always returns `None`.

```rust
use std::error::Error;
use qubit_http::HttpError;
use qubit_retry::RetryError;

fn inspect_retry(error: &HttpError) {
    if let Some(diagnostics) = error.retry_diagnostics() {
        eprintln!("{diagnostics:?}");
    }
    if let Some(retry) = error
        .source()
        .and_then(|source| source.downcast_ref::<RetryError<HttpError>>())
    {
        eprintln!("reason={:?}, attempts={}", retry.reason(), retry.context().attempts());
    }
}
```

## Logging Redaction

HTTP logs use `tracing::trace!`. Both conditions must be true:

1. `options.logging.enabled = true`.
2. The active tracing subscriber enables TRACE.

HTTP request execution uses the `Redactor` captured by the `HttpRequest` as the policy for request and response logs, execution errors, later response reads, and SSE diagnostics. The executing client's logging options control whether and how much it logs. A request built by client A and executed by client B retains A's policy; rebuilding B does not rebind an old request. Build a new request with the client whose policy you need. These policies are not merged. Separately constructed `HttpLogger` values use the policy passed to their constructor.

Request headers, request body, response headers, and response body can be toggled separately. Body logs include only the first `logging.body_size_limit` bytes and show a truncation marker for the remainder. Binary bodies are rendered as `<binary N bytes>`. Unsupported bodies without a structured or textual `Content-Type` are rendered as `<redacted: unsupported HTTP body>`. Request-body logging previews buffered body variants (`bytes_body`, `text_body`, `json_body`, `form_body`, `multipart_body`, and `ndjson_body`); `stream_body` and `streaming_body` are logged as `<skipped: streaming request body>` because the logger does not consume upload streams.

Logs are redacted through the canonical `qubit_redact::Redactor` and one root `RedactionPolicy`. URL username, password, fragment, and sensitive query parameters are masked; the standard policy preserves non-root URL paths for diagnostics, while `RedactionPolicy::strict()` or explicit `UrlPathPolicy::Redact` hides them. JSON/form/multipart body fields use HTTP context views over the same base rules, masking table, and static limits. Multipart file parts remain `<redacted: file part>`; malformed, missing-boundary, or truncated multipart bodies remain fail-closed.

For headers, `http::HeaderValue::is_sensitive()` is a value-level `Secret` declaration. Request, response, streaming-response, and `Debug` rendering honor it before header-name matching. An allow rule cannot expose a marked value; unmarked values continue to use the same immutable name policy snapshot.

`RedactionPolicy::builder()` has empty application rules and the standard floor. Use `RedactionPolicy::default().to_builder()` to extend a default snapshot. Application allow rules cannot bypass an enabled floor; `http.disable_all_floors()` inside the grouped `http(|http| { ... })` view is the explicit HTTP-context escape hatch. Import `RedactionPolicy` and `Redactor` from `qubit_redact`; `qubit_http` does not re-export them. `logging.body_size_limit` is a presentation bound, while structured body parsing remains bounded by JSON and structure limits.

Install the application default with
`Redactor::replace_application_default(Redactor::new(policy))` during startup.
`RedactionPolicy::default()` remains the fixed standard policy.
`HttpClientOptions::new()` snapshots the current default redactor, including an
application default installed before construction. The client creates one
`Redactor` snapshot and preserves it through requests, responses, retries,
interceptors, errors, and SSE diagnostics. Request-builder and options
preflight errors use the snapshot held by their owning object, even if the
application default changes later. For migration, configure one
`RedactionPolicyBuilder` through `fields()`, `http()`, and `limits()`;
`qubit_http` does not re-export these redaction types.

Example:

<!-- guide-check:redaction:start -->
```rust
use http::Method;
use qubit_http::{
    HttpClient,
    HttpClientOptions,
    HttpLogger,
};
use qubit_redact::{RedactionPolicy, Sensitivity};
use serde_json::json;

let mut options = HttpClientOptions::new();
options.logging.enabled = true;
options.logging.log_request_header = true;
options.logging.log_request_body = true;
options.logging.log_response_body = true;
let builder = RedactionPolicy::default().to_builder().http(|http| {
    let _ = http.header().raise("x-api-key", Sensitivity::High);
    let _ = http.query().raise("access_token", Sensitivity::High);
    let _ = http.body().raise("password", Sensitivity::Secret);
})?;
options.log_redaction_policy = builder.build()?;

let client = HttpClient::builder().options(options).build()?;
let request = client
    .request(Method::POST, "https://api.example.com/login")
    .query_param("access_token", "secret-token")
    .header("x-api-key", "secret-key")?
    .json_body(&json!({
        "username": "alice",
        "password": "secret-password",
    }))?
    .build();

let mut response = client.execute(request).await?;
let _ = response.bytes().await?;
let logger = HttpLogger::new(client.options());
logger.log_response(&mut response).await?;
```
<!-- guide-check:redaction:end -->

Automatic response logging runs before `execute` returns. If the body has not been consumed then, the automatic log reports `<skipped: body not yet consumed>`; it does not read the body for logging, even when `Content-Length` is known. Reading the body later does not trigger another log entry. To log a buffered response body explicitly, finish a buffered read and then call `HttpLogger::log_response` as shown above. This call logs the response line and any enabled headers again. Enable TRACE in the application's tracing subscriber as well as the logging switches.

The body preview limit and redaction rules still apply to the explicit log. If `bytes()` fails, `?` returns the read error and the explicit logger call is not reached. Buffering is also subject to the configured read timeout and cancellation token. This pattern is for a finite body that can be buffered within `response_body_size_limit`; do not use it to consume an unbuffered `stream()` or SSE response just for logging. Replace `https://api.example.com/login` with your service URL. The guide checker compiles this example but does not connect to that address. The example uses the same client to build and execute the request, and constructs the logger from that client's options. A separately constructed logger uses the policy passed to its constructor; it does not automatically inherit a different request snapshot.

## Proxy And IPv4-only

Proxying is disabled by default. `use_env_proxy` is also `false` by default, so the client calls `no_proxy()` and ignores environment proxy variables. To inherit `HTTP_PROXY` / `HTTPS_PROXY` and similar variables when no explicit proxy is configured, set `options.use_env_proxy = true` or configure `use_env_proxy = true`. Enabled explicit proxies require host and port.

`proxy.host` accepts only a domain name, IPv4 literal, or complete IPv6 literal (with or without matching square brackets). Keep the port in `proxy.port` and credentials in `username` / `password`; schemes, paths, query strings, userinfo, and embedded ports are rejected.

```rust
use qubit_http::{ProxyType, HttpClientOptions};

let mut options = HttpClientOptions::new();
options.proxy.enabled = true;
options.proxy.proxy_type = ProxyType::Socks5;
options.proxy.host = Some("127.0.0.1".to_string());
options.proxy.port = Some(1080);
```

`ProxyType::Socks5` uses scheme `socks5h`, which performs remote DNS. Setting username enables proxy Basic Auth; setting password without username fails validation.

When `ipv4_only = true`, the local resolver filters DNS results to IPv4, request URLs with IPv6 literal hosts are rejected, explicitly configured IPv6 proxy literals are rejected during client construction, and IPv6 redirect targets retain their existing checks. Environment proxy selection remains delegated to reqwest: an IPv6 environment proxy can still be reached from the client, because the local resolver does not constrain the client-to-proxy connection. `ProxyType::Socks5` uses `socks5h`, so the target hostname is sent to the proxy for remote DNS. HTTP and HTTPS proxies also control target resolution and egress. The local resolver cannot constrain the proxy-to-target address family either, so IPv4-only is not an end-to-end guarantee across a proxy. Deployments that require IPv4 egress through a proxy must enforce it at the proxy, or disable environment proxying and use a verified direct connection.

## SSE Decoding

Choose the SSE API by what you need:

| Goal | Use |
| --- | --- |
| Read raw SSE messages | `response.sse_messages()` |
| Read OpenAI-style JSON chunks or a `[DONE]` marker | `response.sse_chunks::<T>()` |
| Reconnect automatically after a long-lived stream drops | `client.execute_sse_with_reconnect(...)` |

`sse_messages()` and `sse_chunks::<T>()` decode the response body without checking
`Content-Type`. If the upstream protocol requires SSE, verify that the response
declares `text/event-stream` before using these direct decoders. The reconnect
API performs this check itself and returns `SseProtocol` on a missing or invalid
media type. A media-type mismatch uses a fixed error summary and does not
include the original `Content-Type`; match `SseProtocol` and inspect the method,
URL, and status instead of matching the header value in `message`.

SSE message decoding starts from `HttpResponse`:

```rust
use futures_util::StreamExt;
use http::Method;

let response = client
    .execute(client.request(Method::GET, "/stream").build())
    .await?;
response.validate_sse_content_type()?;

let mut messages = response.sse_messages();
while let Some(item) = messages.next().await {
    let message = item?;
    println!(
        "event={:?} last_event_id={:?} data={}",
        message.event, message.last_event_id, message.data
    );
}
```

### Configure `sse_messages` options

`sse_max_line_bytes` and `sse_max_frame_bytes` each return `HttpResponse` (moving `self` back to the caller), so you can chain them with `sse_messages()` **before** consuming the body. `sse_messages()` decodes from `stream()` using the limits stored on that `HttpResponse` instance:

```rust
use futures_util::StreamExt;
use http::Method;

let response = client
    .execute(client.request(Method::GET, "/stream").build())
    .await?;

let mut messages = response
    .sse_max_line_bytes(64 * 1024)      // max bytes per SSE line
    .sse_max_frame_bytes(1024 * 1024) // max bytes per SSE frame
    .sse_messages();
while let Some(item) = messages.next().await {
    let message = item?;
    println!(
        "event={:?} last_event_id={:?} data={}",
        message.event, message.last_event_id, message.data
    );
}
```

`SseMessage` fields:

| Field | Meaning |
| --- | --- |
| `event` | Optional `event:` field |
| `data` | Multiple `data:` lines joined with `\n` |
| `last_event_id` | Current EventSource last-event-id after applying this frame and prior control-only `id:` frames |

Protocol behavior:

- Recognizes CR, LF, and CRLF as line endings.
- Each line must be UTF-8, otherwise `HttpErrorKind::SseProtocol` is returned.
- A blank line is the only event-dispatch boundary. A UTF-8 BOM is ignored only when it begins the first line; BOM characters elsewhere are preserved.
- Comment lines starting with `:` are ignored.
- Unknown fields are ignored.
- `retry:` is accepted only when its value is a non-empty sequence of ASCII digits that fits in `u64`; other values are ignored without replacing a previous valid retry delay.
- `retry:` and control-only `id:` frames are internal control records; they are not exposed as `SseMessage` values.
- EOF does not dispatch an incomplete data-bearing event; complete retry and control-only id records retain their existing control behavior. Servers must terminate each event with a blank line, including the final event; closing the connection does not complete a data frame.
- Default line/frame limits come from `HttpClientOptions`. To override them for one response only, chain `sse_max_line_bytes` / `sse_max_frame_bytes` with `sse_messages()` on the same expression, as shown under “Configure `sse_messages` options” above.

The frame limit counts cumulative UTF-8 bytes of non-empty, non-comment logical lines between blank-line boundaries. Field names, separators, values, and unknown fields count. Line terminators, blank-line separators, and comment lines do not; comment lines remain subject to `max_line_bytes`. The first-line BOM is removed before counting. Reaching the limit is allowed; exceeding it returns `SseProtocol`. This is not a wire-frame, whole-session byte, event-count, or CPU-time limit; the line limit, read timeout, and cancellation retain their separate roles.

### SSE JSON Chunks

The following snippets assume `request` is already built, and that `MyChunk` and `handle` are defined by the caller.

`sse_chunks` takes no arguments: the done-marker policy defaults to `DoneMarkerPolicy::DefaultDone` (the `Default` for `DoneMarkerPolicy`, recognizing trimmed `data:` equal to `[DONE]`), and can be overridden via `HttpClientOptions::sse_done_marker_policy` or `HttpResponse::sse_done_marker_policy`.

JSON SSE completion defaults to `SseCompletionPolicy::AllowEof`. Set
`SseCompletionPolicy::RequireDoneMarker` to require a configured done marker in a complete event. EOF without that marker yields one `SseProtocol` error after decoded data. `AllowEof` accepts a stream of complete events without a marker; it does not dispatch an incomplete final event.
Disabled or empty custom markers are rejected for this policy.

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

`json_value_limits`, `sse_json_mode`, `sse_done_marker_policy`, `sse_max_line_bytes`, and `sse_max_frame_bytes` all return `HttpResponse` (moving `self` back to the caller), so you can chain them with `sse_chunks::<T>()` on the same expression. `sse_chunks` reads the effective options from that same instance:

```rust
use futures_util::StreamExt;
use qubit_budget::json::JsonValueLimits;
use qubit_http::sse::{DoneMarkerPolicy, SseChunk, SseJsonMode};

let response = client.execute(request).await?;

let mut chunks = response
    .json_value_limits(JsonValueLimits::builder().max_nodes(10_000).build())
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

`SseJsonMode::Lenient` skips malformed JSON chunks and continues. Automatic decode errors use a fixed summary and keep the safe `JsonDecodeError` as their source. Lenient DEBUG logs include error kind/stage and event/id presence and byte lengths, never their contents; applications can still read the original message fields and should apply their own logging policy. Applications that matched the former error text or logged event/id values should migrate to checking `HttpErrorKind` and inspecting the `JsonDecodeError` source; record message fields only under an explicit application logging policy. `Strict` returns `HttpErrorKind::SseDecode` on the first malformed JSON chunk. Configure JSON value limits through `HttpClientOptions::json_value_limits` or `json.max_*`, and configure SSE framing under `sse.*`. For one response, chain `json_value_limits`, `sse_json_mode`, `sse_done_marker_policy`, `sse_max_line_bytes`, and `sse_max_frame_bytes` with `sse_chunks::<T>()`, as shown above.

### SSE Auto-reconnect

<!-- guide-check:sse-reconnect:start -->
```rust
use futures_util::StreamExt;
use http::Method;
use qubit_http::sse::SseReconnectOptions;
use qubit_retry::{BackoffPolicy, RetryPolicy};

let request = client.request(Method::GET, "/events").build();
let mut events = client.execute_sse_with_reconnect(
    request,
    SseReconnectOptions {
        retry: RetryPolicy::builder()
            .max_attempts(6) // initial connect + 5 reconnects
            .backoff(BackoffPolicy::exponential(
                std::time::Duration::from_secs(1),
                2.0,
                std::time::Duration::from_secs(30),
            )?)
            .build()?,
        reconnect_on_eof: true,
        honor_server_retry: true,
        server_retry_max_delay: None,
        apply_jitter_to_server_retry: true,
    },
);

while let Some(item) = events.next().await {
    let event = item?;
    println!("{}", event.data);
}
```
<!-- guide-check:sse-reconnect:end -->

The `RetryPolicy` admission budgets cover these phases:

| Phase | Operation time | Total time |
| --- | --- | --- |
| Connection opening and response validation | Counts time spent in completed attempts | Counts elapsed time |
| SSE record consumption, including when the consumer pauses polling | Excluded | Counts elapsed time |
| Reconnect wait | Excluded | Counts elapsed time |

Operation time accumulates time spent in completed connection-opening attempts, not SSE consumption or reconnect waits. Total time includes elapsed consumption and waits; admission limits do not interrupt an active stream. These budgets control whether another connection may start. They are not a hard deadline for a stream already being read. Request and read timeouts and cancellation remain independent controls.

The default reconnect settings are `retry.max_attempts = 4` (that is, at most 3 reconnects), exponential `BackoffPolicy` with `initial=1s`, `max=30s`, `multiplier=2.0`, no jitter, reconnect on EOF, and honor server `retry:`. The latest valid `retry:` delay persists until replaced; use `honor_server_retry = false` to ignore server hints. Server `retry:` delays are capped by `server_retry_max_delay`; when it is `None`, the cap is derived from the backoff policy's maximum delay, or falls back to 30 seconds when no maximum exists. This cap applies only to the SSE `retry:` field. For a reconnectable 429/5xx handshake, the final request's `honor_retry_after` preference controls whether the parsed HTTP hint is passed to the backoff policy; this remains a single-attempt HTTP hint governed by its own policy cap and the reconnect budget. `apply_jitter_to_server_retry` is enabled by default, but server-provided delays remain unchanged unless the supplied `RetryPolicy` uses jitter. Reconnects reuse the original request and disable inner HTTP retry. After a non-empty SSE `id:` field updates the reconnect cursor, including in a control-only record, the next request includes `Last-Event-ID`; an empty ID reset makes the next request omit that header. A 204 response ends the reconnect stream normally without requiring `Content-Type`. These are deliberate behavior changes; callers should handle 204 as normal end-of-stream and servers should send a new valid `retry:` to change the delay. The API does not claim full EventSource compliance. `execute_sse_with_reconnect` requires response `Content-Type` to be `text/event-stream` for other successful statuses. SSE decoder errors include the response method, original request URL, and status while preserving more specific context and the existing error source. Cancellation does not reconnect. SSE protocol errors do not reconnect by default. Retryable timeout, transport, 429/5xx, and unexpected-EOF-like errors may reconnect.

SSE reconnects use the independent `SseReconnectOptions.retry` policy. Ordinary
HTTP retry settings, including `retry.method_policy`, and request-level
`disable_retry()` do not disable outer SSE reconnects. The original request may
therefore be sent repeatedly, including with `POST` or `PATCH`. Prefer `GET`;
for side-effecting operations, confirm that the operation and body are safe to
repeat under a business-level idempotency guarantee. A factory that creates a
fresh body stream provides technical replayability, not business idempotency.
Use `execute` and manage each connection yourself when you need complete control
over replay.

Once the server supplies an event ID, the reconnect runner owns the final `Last-Event-ID` header. Its non-empty cursor wins after default headers, injectors, and request interceptors have been merged; an empty ID removes the header. Before any server ID is received, ordinary header precedence continues to apply. Applications that intentionally control every reconnect cursor should manage individual requests through `execute` instead.

## Configuration Reference

The table below lists every configuration key supported by `HttpClientOptions::from_config`. If you pass `config.section("http")`, these keys are read relative to that prefix.

| Key | Description |
| --- | --- |
| `base_url` | Base URL used to resolve relative request paths |
| `ipv4_only` | Keeps only IPv4 DNS results and rejects IPv6 literal URLs |
| `error_response_preview_limit` | Body preview byte limit stored on non-2xx errors; defaults to 16 KiB |
| `response_body_size_limit` | Maximum bytes accumulated by whole-response body and JSON helpers; defaults to 8 MiB |
| `error_response_body_limit` | Maximum raw bytes retained in non-2xx `HttpStatusResponse` values; defaults to 64 KiB and is separate from the redacted `error_response_preview_limit` (16 KiB by default) |
| `user_agent` | Default User-Agent passed to the reqwest builder |
| `max_redirects` | Redirect limit |
| `pool_idle_timeout` | Connection pool idle timeout |
| `pool_max_idle_per_host` | Max idle connections per host |
| `use_env_proxy` | Whether to inherit environment proxies when explicit proxying is disabled; defaults to `false` |
| `log_redaction.url_path_policy` | URL path policy: `preserve` by default, or explicit `redact` |
| `log_redaction.sensitive_headers` | String list added to the default sensitive-header set |
| `log_redaction.sensitive_query_params` | String list added to the default sensitive-query-parameter set |
| `log_redaction.sensitive_body_fields` | String list added to the default sensitive-body-field set |
| `log_redaction.excluded_sensitive_headers` | String list of exact header names explicitly allowed to remain visible |
| `log_redaction.excluded_sensitive_query_params` | String list of exact query-parameter names explicitly allowed to remain visible |
| `log_redaction.excluded_sensitive_body_fields` | String list of exact structured-body field names explicitly allowed to remain visible |
| `origin_policy` | `same_origin` (default) checks initial URLs using the request snapshot; redirects use the executing client backend policy. `any_origin` permits cross-origin URLs at the stage controlled by that policy; see [origin policy across clients](#origin-policy-across-clients) |
| `default_headers` | JSON map string of default request headers; cannot be combined with `default_headers.<name>`; case-insensitive duplicate names are rejected |
| `default_headers.<name>` | One default request header subkey; cannot be combined with the `default_headers` JSON map; case-insensitive duplicate names are rejected |
| `json.max_depth` | Maximum JSON nesting depth; defaults to `128` |
| `json.max_nodes` | Maximum total JSON nodes; defaults to `100000` |
| `json.max_sequence_items` | Maximum items in one JSON array; defaults to `100000` |
| `json.max_map_entries` | Maximum entries in one JSON object; defaults to `100000` |
| `json.max_key_bytes` | Maximum UTF-8 bytes in one object key; defaults to `65536` |
| `json.max_string_bytes` | Maximum UTF-8 bytes in one string; defaults to `8388608` |
| `json.max_number_bytes` | Maximum bytes in one number lexeme; defaults to `4096` |
| `json.max_payload_bytes` | Maximum cumulative key, string, and number payload bytes; defaults to `8388608` |
| `json.max_output_bytes` | Maximum aggregate encoded JSON/NDJSON request-body bytes, including NDJSON line terminators; defaults to `8388608` |
| `timeouts.connect_timeout` | Connect timeout |
| `timeouts.read_timeout` | Bounds each normal response-body read and the complete non-success error-body capture phase |
| `timeouts.response_header_timeout` | Cumulative asynchronous preparation and send deadline through response headers; synchronous request interceptors run before this deadline starts |
| `timeouts.request_timeout` | Optional reqwest per-attempt network request and response-body deadline; excludes async header injection, streaming-body factory preparation, retry waits, and other attempts |
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
| `retry.max_duration` | Optional soft continuation budget, including attempts, waits, and retry control callbacks |
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
| `sse.done_marker` | `DISABLED` or `DISABLE` disables done markers; `DEFAULT` uses `[DONE]`; any other non-empty string becomes a `Custom` marker compared to trimmed `data:` text |
| `sse.completion` | `allow_eof` (default) or `require_done_marker`; the latter requires an enabled non-empty `sse.done_marker` |
| `sse.max_line_bytes` | SSE single-line byte limit |
| `sse.max_frame_bytes` | Maximum cumulative UTF-8 bytes of non-empty, non-comment logical lines between blank-line boundaries; unknown fields count, terminators and comments do not. Comment lines remain subject to `sse.max_line_bytes`. This is not a wire-size or whole-session limit. |

## Limitations And Best Practices

- Enable global retry for read-only or idempotent APIs. For POST/PATCH, retry must be enabled and the effective method policy must allow replay, for example with `AllMethods`. Per-request `force_retry()` enables retry but does not override the method policy or `max_attempts`. Allow replay only when the operation is safe to repeat.
- Set a realistic `read_timeout` for long-lived streams/SSE; too short a value turns a slow but healthy stream into `ReadTimeout`.
- Automatic TRACE response-body logging runs before `execute` returns and reports only bodies already buffered at that point. A later body read does not add a log entry; call `HttpLogger::log_response` explicitly after buffering when needed. That call repeats the response line and any enabled headers. It does not consume an unbuffered stream or SSE body.
- Keep `proxy.enabled = false` and `use_env_proxy = false` when you want proxying fully disabled; explicitly enable `use_env_proxy` when environment proxy inheritance is desired.
- Prefer passing a scoped `section("http")` to `HttpClient::from_config`, so error paths preserve useful context.

The crate deliberately does not expose every `reqwest` API, does not retry
stream-read failures after `HttpResponse` has been returned, and does not
consume an unbuffered response body merely to produce a TRACE log. Reading it later does
not trigger a second log; use `HttpLogger::log_response` after a successful buffered
read when an additional response entry is needed. Whole-body
and JSON helpers are bounded by configured limits; SSE line and frame limits
also apply while decoding. Treat buffered request bodies and fresh
`streaming_body` factories as replayable only when the application can safely
repeat the operation.

## Upgrade from 0.15 to 0.16

This section compares the repository's `v0.15.1` tag with `0.16.0` at commit `e93a4cb`. The crate's declared MSRV remains Rust 1.94. Update the dependency requirement to `qubit-http = "0.16"`; a `^0.15` requirement does not admit `0.16`.

| Area | Verified change or retained contract | Caller action |
| --- | --- | --- |
| Dependencies | The manifest updates `qubit-json` 0.10 to 0.12, `qubit-config` 0.14 to 0.15, `qubit-budget` 0.7 to 0.10, `qubit-redact` 0.9 to 0.10, and Tokio 1.53 to 1.54. `reqwest` remains on 0.13. | Review your lockfile and direct dependency constraints for the versions required by your chosen `qubit-http` release. |
| Base URL | `base_url` remains the configuration key, and validation now accepts only HTTP or HTTPS URLs. No replacement key was introduced. | Check configured base URLs for schemes such as `file:` or `ws:`; provide an HTTP(S) API base URL. |
| Configuration | The compared source does not identify a renamed `retry`, `timeouts`, or `sse` configuration key. Current guides list the supported keys and accepted values. | Parse your actual configuration with `HttpClientOptions::from_config` or `HttpClient::from_config(...)`; treat unknown-key or invalid-value errors as actionable migration findings. |
| Retry ownership | `execute` retries eligible failures before returning `HttpResponse`; later response-body and SSE reads are caller-owned and do not re-enter HTTP retry. An outer SDK may own a separate operation-level retry. | Keep request-level retry disabled when an SDK already retries the whole operation, as the OpenAI provider does with `disable_retry()`. Test that one application attempt does not multiply retries unexpectedly. |
| Client hooks | In both versions, `to_builder()` copies options only; `rebuild_with_options()` creates a new backend and preserves registered injectors and interceptors. | When replacing options on a configured client, use `rebuild_with_options()` if hooks are part of the contract; do not assume `to_builder()` carries them. |
| Options struct literals | `HttpClientOptions` has the new `extra_root_certificates_der` field. | Add the field to exhaustive struct literals. Literals using `..HttpClientOptions::default()` need no change. |
| SSE | The 0.16 source adds explicit handling for empty `id:` resets, persistent valid SSE `retry:` hints, and normal termination on `204 No Content` before media-type validation. Strict JSON and done-marker policies remain caller-selected. | Check cursor reset and retry timing expectations. For strict JSON streams, explicitly use `SseJsonMode::Strict` and `SseCompletionPolicy::RequireDoneMarker`, then test complete and incomplete frames. |

To build a client from configuration, use this complete example. It only constructs the client; it does not send a network request. For the supported keys and configuration behavior, see [Loading From qubit-config](#loading-from-qubit-config).

```rust
use qubit_config::Config;
use qubit_http::HttpClient;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::new();
    let _client = HttpClient::from_config(&config)?;
    Ok(())
}
```

To verify a downstream SDK upgrade, check ordinary JSON response decoding, one network attempt when the SDK owns retries, strict SSE completion, and hook retention after `rebuild_with_options()`. A small Cargo consumer can exercise these call patterns; that check does not establish that every downstream package has been upgraded.


## Further Reading

- [README](../README.md) and [中文 README](../README.zh_CN.md)
- [中文用户指南](user_guide.zh_CN.md)
- [Design](design.md) and [中文设计文档](design.zh_CN.md)
- [API Reference](https://docs.rs/qubit-http)
