# qubit-http 用户指南

本文档基于当前源码和测试整理，适用于 crate `qubit-http` 0.3.1，Rust 代码中通过库名 `qubit_http` 使用。

`qubit-http` 是一个异步 HTTP 客户端基础设施库。它封装 `reqwest`，提供统一的客户端配置、请求构建、响应读取、错误分类、TRACE 日志脱敏、自动重试、代理、IPv4-only 解析、请求/响应拦截器，以及 Server-Sent Events（SSE）解码和重连能力。

## 安装与导入

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

## 快速开始

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

`execute` 只把 2xx 状态码视为成功。非 2xx 状态会返回 `HttpErrorKind::Status`，并在错误中携带状态码、方法、URL、截断后的响应体预览，以及可解析的 `Retry-After`。

## 创建客户端

### 默认客户端

```rust
let client = qubit_http::HttpClientFactory::new().create_default()?;
```

默认行为：

| 项目 | 默认值 |
| --- | --- |
| `base_url` | 无，必须使用绝对 URL，或在请求级设置 base URL |
| 连接超时 | 10 秒 |
| 读超时 | 120 秒 |
| 写超时 | 120 秒 |
| 整体请求超时 | 无 |
| 代理 | 禁用，并调用 `reqwest` 的 `no_proxy()`，不会继承环境代理 |
| 日志 | 开启，但只有 tracing TRACE 级别启用时才输出 |
| 日志体预览 | 16 KiB |
| 非成功响应体预览 | 16 KiB |
| 自动重试 | 禁用 |
| 重试最大尝试次数 | 3，含第一次请求 |
| 重试方法策略 | 只允许幂等方法 |
| 敏感头 | 内置一组常见认证/密钥类头名 |
| IPv4-only | 关闭 |
| SSE JSON 模式 | `Lenient` |
| SSE 单行上限 | 64 KiB |
| SSE 单帧上限 | 1 MiB |

### 使用代码配置

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

let client = HttpClientFactory::new().create(options)?;
```

`create` 会先执行校验。常见校验包括：超时必须大于 0；启用代理时必须有非空 host 和非 0 port；只有设置 username 时才能设置 password；日志记录请求体或响应体时 `body_size_limit` 必须大于 0；`user_agent` 不能为空并且必须是合法 header value；SSE 行/帧上限必须大于 0。

### 从 qubit-config 读取

`HttpClientOptions::from_config` 和 `HttpClientFactory::create_from_config` 接收任意 `qubit_config::ConfigReader`。如果传入 `config.prefix_view("http")`，下面表格中的键都按相对路径读取。

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

支持的配置键：

| 键 | 说明 |
| --- | --- |
| `base_url` | 相对请求路径的基础 URL |
| `ipv4_only` | 启用后 DNS 只保留 IPv4 地址，并拒绝 IPv6 literal URL |
| `error_response_preview_limit` | 非 2xx 错误中保留的响应体预览字节数 |
| `user_agent` | 传给 `reqwest` builder 的默认 User-Agent |
| `max_redirects` | 最大重定向次数 |
| `pool_idle_timeout` | 连接池空闲超时 |
| `pool_max_idle_per_host` | 每个 host 最大空闲连接数 |
| `sensitive_headers` | 覆盖默认敏感头集合的字符串列表 |
| `timeouts.connect_timeout` | 连接超时 |
| `timeouts.read_timeout` | 读取响应体或流时的单次等待超时 |
| `timeouts.write_timeout` | 发送请求的写阶段超时 |
| `timeouts.request_timeout` | 整体请求超时，可选 |
| `proxy.enabled` | 是否启用代理 |
| `proxy.proxy_type` | `http`、`https`、`socks5` 或 `socks5h` |
| `proxy.host` | 代理主机 |
| `proxy.port` | 代理端口 |
| `proxy.username` | 代理 Basic Auth 用户名 |
| `proxy.password` | 代理 Basic Auth 密码，必须配合 username |
| `logging.enabled` | 是否允许 TRACE HTTP 日志 |
| `logging.log_request_header` | 是否记录请求头 |
| `logging.log_request_body` | 是否记录请求体预览 |
| `logging.log_response_header` | 是否记录响应头 |
| `logging.log_response_body` | 是否记录响应体预览 |
| `logging.body_size_limit` | 日志体预览字节数 |
| `retry.enabled` | 是否启用内置重试 |
| `retry.max_attempts` | 最大尝试次数，含第一次请求 |
| `retry.max_duration` | 总重试耗时上限，可选 |
| `retry.delay_strategy` | `NONE`、`FIXED`、`RANDOM`、`EXPONENTIAL_BACKOFF` 或 `EXPONENTIAL` |
| `retry.fixed_delay` | 固定延迟 |
| `retry.random_min_delay` | 随机延迟下限 |
| `retry.random_max_delay` | 随机延迟上限 |
| `retry.backoff_initial_delay` | 指数退避初始延迟 |
| `retry.backoff_max_delay` | 指数退避最大延迟 |
| `retry.backoff_multiplier` | 指数退避倍率 |
| `retry.jitter_factor` | 抖动比例，范围 `0.0..=1.0` |
| `retry.method_policy` | `IDEMPOTENT_ONLY`/`IDEMPOTENT`、`ALL_METHODS`/`ALL`、`NONE`/`DISABLED` |
| `retry.status_codes` | 重试状态码白名单；未配置时默认重试 429 和 5xx |
| `retry.error_kinds` | 非状态错误类型白名单；未配置时默认重试超时和 transport |
| `sse.json_mode` | `LENIENT` 或 `STRICT` |
| `sse.max_line_bytes` | SSE 单行字节上限 |
| `sse.max_frame_bytes` | SSE 单帧字节上限 |

`default_headers` 支持两种形式。优先使用子键形式：

```rust
config.set("http.default_headers.authorization", "Bearer token".to_string())?;
config.set("http.default_headers.x-request-id", "abc-123".to_string())?;
```

如果没有任何 `default_headers.*` 子键，也可以在 `default_headers` 写 JSON map 字符串：

```rust
config.set(
    "http.default_headers",
    r#"{"x-app-id":"demo","x-version":"1.0"}"#.to_string(),
)?;
```

## 构建请求

通过 `client.request(method, path)` 创建 `HttpRequestBuilder`。`path` 可以是绝对 URL，也可以是相对路径；绝对 URL 会绕过 `base_url`，相对路径必须能和 `base_url` join。

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

请求体构建方法：

| 方法 | 行为 |
| --- | --- |
| `bytes_body` | 原始字节体，不自动设置 `Content-Type` |
| `stream_body` | 按顺序发送字节块，走 reqwest streaming body |
| `text_body` | 文本体；缺少 `Content-Type` 时设置 `text/plain; charset=utf-8` |
| `json_body` | 序列化 JSON；缺少 `Content-Type` 时设置 `application/json` |
| `form_body` | `application/x-www-form-urlencoded` |
| `multipart_body` | 原始 multipart 字节；需要非空 boundary，缺少 `Content-Type` 时设置 multipart |
| `ndjson_body` | 每条记录一行 JSON；缺少 `Content-Type` 时设置 `application/x-ndjson` |

请求级覆盖：

| 方法 | 用途 |
| --- | --- |
| `timeout` | 覆盖整体请求超时 |
| `write_timeout` | 覆盖发送阶段超时 |
| `read_timeout` | 覆盖响应体读取/流读取超时 |
| `base_url` / `clear_base_url` | 覆盖或清除本次请求 base URL |
| `ipv4_only` | 覆盖本次请求的 IPv4-only URL 校验 |
| `cancellation_token` | 绑定 `CancellationToken`，发送前、发送中、读 body/stream 时都会检查 |
| `force_retry` | 本次请求强制启用重试 |
| `disable_retry` | 本次请求禁用重试 |
| `retry_method_policy` | 覆盖本次请求的可重试 HTTP 方法策略 |
| `honor_retry_after` | 本次请求在 429/5xx 且可重试时尊重 `Retry-After` |

## Header、注入器和拦截器

最终请求头合并顺序如下，后者覆盖同名 header：

1. 创建 builder 时快照下来的客户端默认 header。
2. 同步 `HttpHeaderInjector`，按注册顺序执行。
3. 异步 `AsyncHttpHeaderInjector`，按注册顺序执行。
4. 请求本身设置的 header。

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

请求拦截器在每次尝试发送前执行，可以修改 `HttpRequest`，返回错误会短路本次执行。响应拦截器只在成功状态响应上执行，可以检查或修改 `HttpResponseMeta`，返回错误会让 `execute` 失败。

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

## 读取响应

`HttpResponse` 暴露 `meta()`、`status()`、`headers()`、`url()`、`request_url()`、`is_success()`、`retry_after_hint()` 和 body 读取方法。

```rust
let mut response = client.execute(request).await?;
let text = response.text().await?;
```

body 可用方式：

| 方法 | 行为 |
| --- | --- |
| `bytes_body()` | 懒读取完整 body（首次 `await` 时读入并缓存），后续可重复调用 |
| `text()` | 基于 `bytes_body()`，用 UTF-8 解码完整 body |
| `json<T>()` | 基于 `bytes_body()`，用 serde JSON 反序列化完整 body |
| `stream_body()` | 返回 `HttpByteStream`；若 body 已缓存则为单块内存流。未缓存时调用本身**不会**立刻读完整 body，而是把底层响应交给返回的流，**在轮询该流时**按需读取字节块 |

注意：底层 `reqwest` 响应体在同一 `HttpResponse` 上只能有一条消费路径。调用 `bytes_body`、`text` 或 `json` 会把完整 body 读入并缓存；之后 `stream_body` 会返回由缓存构成的单块流。若在未缓存时先调用 `stream_body`，底层句柄已交给返回的流，须通过该流读完 body；此时再调用 `bytes_body` / `text` / `json` 不会再从网络补读（会得到空 body），因此不要混用「先流式、再整包读」。

`retry_after_hint()` 会在响应状态为 429 或 5xx 且存在合法 `Retry-After` header 时返回延迟。它支持 `delta-seconds` 和 HTTP-date 两种格式；HTTP-date 早于当前时间时返回 0 秒。`HttpResponseMeta` 上也有同名方法，响应拦截器可以在只拿到 metadata 时读取这个提示。

## 错误模型

所有运行时 HTTP 错误使用 `HttpError`，结果别名是 `HttpResult<T> = Result<T, HttpError>`。

`HttpError` 包含：

| 字段 | 含义 |
| --- | --- |
| `kind` | 错误分类 |
| `method` | 可选 HTTP 方法 |
| `url` | 可选请求 URL |
| `status` | 可选响应状态码 |
| `message` | 人类可读错误信息 |
| `response_body_preview` | 非 2xx 响应体预览 |
| `retry_after` | 从 `Retry-After` 解析出的延迟 |
| `source` | 底层错误 |

错误分类：

```rust
InvalidUrl, BuildClient, ProxyConfig,
ConnectTimeout, ReadTimeout, WriteTimeout, RequestTimeout,
Transport, Status, Decode, SseProtocol, SseDecode,
Cancelled, Other
```

`retry_hint()` 会把超时、transport、429 和 5xx 状态视为可重试提示，其余默认不可重试。真正是否重试还要结合 `HttpRetryOptions` 和方法策略。

## 自动重试

自动重试默认关闭。开启后，只有当 `retry.enabled = true`、`max_attempts > 1`，并且方法策略允许当前 HTTP 方法时，`execute` 才会进入重试流程。

默认重试条件：

| 类型 | 默认可重试 |
| --- | --- |
| 状态码 | 429 和所有 5xx |
| 非状态错误 | connect/read/write/request timeout 和 transport |
| 方法 | GET、HEAD、PUT、DELETE、OPTIONS、TRACE |

可以用 `retry.status_codes` 和 `retry.error_kinds` 配置白名单。白名单一旦设置，就只重试列出的状态码或错误类型。

对单个请求可以覆盖：

```rust
let request = client
    .request(Method::POST, "/submit")
    .force_retry()
    .retry_method_policy(qubit_http::HttpRetryMethodPolicy::AllMethods)
    .honor_retry_after(true)
    .build();
```

`honor_retry_after(true)` 只在请求级启用。遇到可重试的 429 或 5xx 时，如果响应里有 `Retry-After`，重试控制器会确保下一次尝试至少等待该 header 指定的时间。

## 日志与敏感头

HTTP 日志使用 `tracing::trace!`。必须同时满足：

1. `options.logging.enabled = true`。
2. 当前 tracing subscriber 开启 TRACE 级别。

可分别控制请求头、请求体、响应头、响应体。body 只记录前 `logging.body_size_limit` 字节，超出部分显示截断提示；二进制体显示为 `<binary N bytes>`。

敏感 header 会脱敏。默认内置常见认证、token、cookie、secret、password 类 header 名。短值整体显示为 `****`；长值保留前后各 2 个字符，中间替换为 `****`。配置 `sensitive_headers` 会用自定义集合替换默认集合；代码里也可以通过 `SensitiveHttpHeaders` 自行维护集合。

注意：如果 TRACE 日志开启且 `log_response_body = true`，`execute` 会读取并缓存完整响应体后再返回响应，因此这类响应不再保持未消费的后端流。

## 代理与 IPv4-only

代理默认禁用；禁用时客户端调用 `no_proxy()`，不会使用环境变量代理。启用代理必须设置 host 和 port。

```rust
use qubit_http::{ProxyType, HttpClientOptions};

let mut options = HttpClientOptions::new();
options.proxy.enabled = true;
options.proxy.proxy_type = ProxyType::Socks5;
options.proxy.host = Some("127.0.0.1".to_string());
options.proxy.port = Some(1080);
```

`ProxyType::Socks5` 使用 scheme `socks5h`，即远端 DNS。设置 username 后会启用代理 Basic Auth；只设置 password 不设置 username 会校验失败。

`ipv4_only = true` 时：

- `reqwest` DNS resolver 只返回 IPv4 地址。
- URL 中的 IPv6 literal host 会在解析阶段被拒绝。
- 代理 host 如果是 IPv6 literal，也会被拒绝。

## SSE 解码

SSE 事件解码从 `HttpResponse` 开始：

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

`SseEvent` 字段：

| 字段 | 含义 |
| --- | --- |
| `event` | `event:` 字段，可选 |
| `data` | 多行 `data:` 用 `\n` 拼接 |
| `id` | `id:` 字段，可选 |
| `retry` | 合法 `retry:` 毫秒值，可选 |

协议行为：

- 按 `\n` 分行，剥离行尾 `\r`。
- 每行必须是 UTF-8，否则返回 `HttpErrorKind::SseProtocol`。
- 空行 flush 一个 event。
- 注释行（以 `:` 开头）忽略。
- 未知字段忽略。
- `retry:` 只有能解析为 `u64` 时才记录。
- 流结束时如果还有未 flush 字段，会输出最后一个 event。
- 单行和单帧上限来自客户端配置，也可调用 `decode_sse_events_with_limits` 覆盖。

### SSE JSON chunk

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

`DoneMarkerPolicy` 支持：

- `Disabled`：不识别完成标记。
- `DefaultDone`：`data:` 去除空白后等于 `[DONE]` 时输出 `SseChunk::Done` 并结束。
- `Custom(String)`：使用自定义完成标记。

`SseJsonMode::Lenient` 会跳过 malformed JSON chunk 并继续；`Strict` 会在第一个 malformed JSON 处返回 `HttpErrorKind::SseDecode`。可以在 `HttpClientOptions` 中设置默认模式，也可以调用 `decode_sse_json_chunks_with_mode` 或 `decode_sse_json_chunks_with_mode_and_limits` 做请求级读取覆盖。

### SSE 自动重连

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

默认重连配置为最多重连 3 次，基础延迟 1 秒，EOF 后重连，并尊重服务端 `retry:`。重连时会复用原始请求；如果之前收到过 SSE `id:`，下一次请求会带上 `Last-Event-ID` header。取消错误不会触发重连；SSE 协议错误默认也不会重连；超时、transport、429/5xx 等 retryable 错误，以及包含 unexpected EOF 语义的错误可触发重连。

## 实用建议

- 对只读或幂等接口开启全局重试；对 POST/PATCH 只有在业务允许重放时才使用 `AllMethods` 或请求级强制重试。
- 对长连接/SSE 设置合理的 `read_timeout`；过短会把正常的慢流误判为 `ReadTimeout`。
- 开启 TRACE 响应体日志会预读完整 body；需要真正流式消费时，建议关闭 `logging.log_response_body`。
- 需要完全禁用代理时保持默认 `proxy.enabled = false`，库不会继承环境代理。
- 如果使用 `from_config`，优先传入 `prefix_view("http")` 一类的作用域视图，这样错误路径会保留完整上下文。
