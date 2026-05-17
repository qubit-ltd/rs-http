# qubit-http 用户指南

本文档基于当前源码和测试整理，适用于 crate `qubit-http` 0.8，Rust 代码中通过库名 `qubit_http` 使用。

`qubit-http` 是一个异步 HTTP 客户端基础设施库。它封装 `reqwest`，提供统一的客户端配置、请求构建、响应读取、错误分类、TRACE 日志脱敏、自动重试、代理、IPv4-only 解析、请求/响应拦截器，以及 Server-Sent Events（SSE）解码和重连能力。

## 如何阅读本文

| 目标 | 建议阅读 |
| --- | --- |
| 第一次接入 | 「快速开始」「构建请求」「读取响应」 |
| 配置客户端 | 「创建客户端」「从 qubit-config 读取」「配置参考」 |
| 排查失败 | 「错误模型」「自动重试」「日志脱敏」 |
| 使用流式响应或 SSE | 「读取响应」「SSE 解码」 |

## 安装与导入

```toml
[dependencies]
qubit-http = "0.8"
http = "1.4"
qubit-config = "0.9"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "sync"] }
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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
| 代理 | 显式代理禁用，且 `use_env_proxy = false`，因此调用 `reqwest` 的 `no_proxy()`，不会继承环境代理 |
| 日志 | 开启，但只有 tracing TRACE 级别启用时才输出 |
| 日志体预览 | 16 KiB |
| 非成功响应体预览 | 16 KiB |
| 自动重试 | 禁用 |
| 重试最大尝试次数 | 3，含第一次请求 |
| 重试方法策略 | 只允许幂等方法 |
| 日志脱敏 | 内置一组常见认证/密钥类 header、query 参数和 body 字段名 |
| IPv4-only | 关闭 |
| SSE JSON 模式 | `Lenient` |
| SSE 完成标记策略 | `DefaultDone`，即识别 `[DONE]` |
| SSE 单行上限 | 64 KiB |
| SSE 单帧上限 | 1 MiB |

### 使用代码配置

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

`create` 会先执行校验。常见校验包括：超时必须大于 0；启用代理时必须有非空 host 和非 0 port；只有设置 username 时才能设置 password；日志记录请求体或响应体时 `body_size_limit` 必须大于 0；`retry.max_attempts` 必须大于 0；`retry.jitter_factor` 必须在 `0.0..=1.0`；`error_response_preview_limit` 必须大于 0；`user_agent` 不能为空并且必须是合法 header value；SSE 行/帧上限必须大于 0。

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

常用配置键：

| 键 | 说明 |
| --- | --- |
| `base_url` | 相对请求路径的基础 URL |
| `timeouts.connect_timeout` | 连接超时 |
| `timeouts.read_timeout` | 读取响应体或流时的单次等待超时 |
| `timeouts.write_timeout` | 发送前准备和发送阶段超时 |
| `timeouts.request_timeout` | 整体请求超时，可选 |
| `proxy.enabled` | 是否启用代理 |
| `use_env_proxy` | 显式代理禁用时，是否继承环境变量代理 |
| `logging.enabled` | 是否允许 TRACE HTTP 日志 |
| `log_sanitize.sensitive_headers` | 追加到默认集合的敏感 header 名称 |
| `log_sanitize.sensitive_query_params` | 追加到默认集合的敏感 query 参数名称 |
| `log_sanitize.sensitive_body_fields` | 追加到默认集合的敏感 JSON/form/multipart body 字段名称 |
| `retry.enabled` | 是否启用内置重试 |
| `retry.max_attempts` | 最大尝试次数，含第一次请求 |
| `retry.delay_strategy` | `NONE`、`FIXED`、`RANDOM`、`EXPONENTIAL_BACKOFF` 或 `EXPONENTIAL` |
| `retry.method_policy` | `IDEMPOTENT_ONLY`/`IDEMPOTENT`、`ALL_METHODS`/`ALL`、`NONE`/`DISABLED` |
| `retry.status_codes` | 重试状态码白名单；未配置时默认重试 429 和 5xx |
| `retry.error_kinds` | 非状态错误类型白名单；未配置时默认重试超时和 transport |
| `sse.json_mode` | `LENIENT` 或 `STRICT` |
| `sse.done_marker` | `DISABLED`（或 `DISABLE`）禁用完成标记；`DEFAULT` 使用 `[DONE]`；其它非空字符串视为自定义完成标记（`Custom`），与 trim 后的 `data:` 文本比较 |
| `sse.max_line_bytes` | SSE 单行字节上限 |
| `sse.max_frame_bytes` | SSE 单帧字节上限 |

完整配置键见文末「配置参考」。

`default_headers` 支持两种形式，但不能同时使用。子键形式如下：

```rust
config.set("http.default_headers.authorization", "Bearer token".to_string())?;
config.set("http.default_headers.x-request-id", "abc-123".to_string())?;
```

也可以在 `default_headers` 写 JSON map 字符串；若同时配置 JSON map 和 `default_headers.*` 子键，解析会失败：

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
    .request_timeout(Duration::from_secs(10))?
    .read_timeout(Duration::from_secs(30))?
    .build();
```

`build()` 后可以调用 `HttpRequest::resolved_url()` 取得真实发送 URL：绝对 `path` 会保持绝对 URL；相对路径会和 `base_url` join；`path` 中已有的 query 会保留，builder 或拦截器追加的 query 会按顺序继续追加。

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

请求体构建方法：

| 方法 | 行为 |
| --- | --- |
| `bytes_body` | 原始字节体，不自动设置 `Content-Type` |
| `stream_body` | 把已在内存中的有序字节块作为 reqwest streaming body 发送 |
| `streaming_body` | 设置异步工厂，每次发送尝试生成新的字节流；适合真正流式上传，也让重试可以重建上传流 |
| `text_body` | 文本体；缺少 `Content-Type` 时设置 `text/plain; charset=utf-8` |
| `json_body` | 序列化 JSON；缺少 `Content-Type` 时设置 `application/json` |
| `form_body` | `application/x-www-form-urlencoded` |
| `multipart_body` | 原始 multipart 字节；需要 1 到 70 个 token-safe 字符的 boundary；缺少 `Content-Type` 时设置默认 multipart，已有 multipart `Content-Type` 但缺少 boundary 时补齐，已有非 UTF-8、非 multipart、boundary 畸形或 boundary 不一致时拒绝 |
| `ndjson_body` | 每条记录一行 JSON；缺少 `Content-Type` 时设置 `application/x-ndjson` |

请求级覆盖：

| 方法 | 用途 |
| --- | --- |
| `request_timeout` | 覆盖整体请求超时（reqwest 单次请求的 deadline） |
| `write_timeout` | 覆盖发送前准备和发送阶段超时 |
| `read_timeout` | 覆盖响应体读取/流读取超时 |
| `base_url` / `clear_base_url` | 覆盖或清除本次请求 base URL |
| `ipv4_only` | 覆盖本次请求的 IPv4-only URL 校验 |
| `cancellation_token` | 绑定 `CancellationToken`，发送前、发送中、读 body/stream 时都会检查 |
| `force_retry` | 本次请求强制启用重试 |
| `disable_retry` | 本次请求禁用重试 |
| `retry_method_policy` | 覆盖本次请求的可重试 HTTP 方法策略 |
| `honor_retry_after` | 本次请求在 429/5xx 且可重试时尊重 `Retry-After` |

请求级 timeout 覆盖会拒绝零时长，并返回 `HttpError`。

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

异步注入器适合在发送前动态获取会变化的请求头，例如认证 token。下面示例中，`TokenProvider` 会复用未过期 token；过期时异步刷新，并把最新值写入 `Authorization`。如果启用了自动重试，异步注入器会在每次发送尝试前重新执行，因此重试请求也能拿到新的 token。

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
    // 调用认证服务、读取安全存储或执行其它异步刷新逻辑。
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

请求拦截器在每次尝试发送前执行，可以修改 `HttpRequest`，返回错误会短路本次执行。响应拦截器只在成功状态响应上执行，接收 `HttpResponseInterceptorContext`：status 和请求方法不可变，响应 headers 和最终响应 URL 可以修改。返回错误会让 `execute` 失败。

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

## 读取响应

`HttpResponse` 暴露 `meta()`、`status()`、`headers()`、`url()`、`request_url()`、`is_success()`、`retry_after_hint()` 以及读 body / 解码 SSE 的 API。`client.execute(...).await?` 得到 `mut response` 后，按下表选择其一消费响应体；**同一 `HttpResponse` 上底层 body 只能走一条路径**（细则见表后「注意」）。表格下方 **「使用示例」** 小节给出表中各方法的典型写法。

body 可用方式：

| 方法 | 行为 |
| --- | --- |
| `bytes()` | 懒读取完整 body（首次 `await` 时读入并缓存），后续可重复调用 |
| `text()` | 基于 `bytes()`，用 UTF-8 解码完整 body |
| `json<T>()` | 基于 `bytes()`，用 serde JSON 反序列化完整 body |
| `stream()` | 返回 `HttpByteStream`；若 body 已缓存则为单块内存流。未缓存时调用本身**不会**立刻读完整 body，而是把底层响应交给返回的流，**在轮询该流时**按需读取字节块 |
| `sse_messages()` | 消费 `self`，按当前 SSE 行/帧上限等选项将 body 解码为 SSE 消息流（详见下文「SSE 解码」） |
| `sse_chunks::<T>()` | 消费 `self`，将 SSE `data:` JSON chunk 解码为 `SseChunk<T>` 流（JSON 模式、完成标记等见下文「SSE JSON chunk」） |

### 使用示例

**整包读取（`bytes` / `text` / `json`）**：三者都会把完整 body 读入并缓存；下面用三次独立请求各演示一种（同一响应上不要混用多种整包/流式路径）。

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

**流式字节（`stream`）**：在未对同一响应调用过 `bytes` / `text` / `json` 的前提下，通过返回的流按块读取；`?` 在拿到流时展开，例如请求上的取消 token 已在开始读取 body 前触发时会在这里返回错误。

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

**SSE（`sse_messages` / `sse_chunks`）**：二者都会**消费** `self`（转移所有权），内部仍基于同一条 body 流；若要在解码前调整行/帧上限、JSON 模式、完成标记等，可将各 `sse_*` 配置方法与 `sse_messages()` / `sse_chunks::<T>()` 写在同一条链上（完整说明见下文「SSE 解码」「SSE JSON chunk」）。

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

注意：底层 `reqwest` 响应体在同一 `HttpResponse` 上只能有一条消费路径。调用 `bytes`、`text` 或 `json` 会把完整 body 读入并缓存；之后 `stream` 会返回由缓存构成的单块流。若在未缓存时先调用 `stream`，底层句柄已交给返回的流，须通过该流读完 body；此时再调用 `bytes` / `text` / `json` 不会再从网络补读（会得到空 body），因此不要混用「先流式、再整包读」。`sse_messages` / `sse_chunks` 也会走这条路径（内部基于 `stream`），且调用后 `HttpResponse` 已被消费，不能再对同一对象调用其它读 body 方法。

| 可以 | 避免 |
| --- | --- |
| `bytes()` / `text()` / `json()` 读取完整响应体并复用缓存 | 先 `stream()`，再对同一响应调用 `bytes()` / `text()` / `json()` |
| `bytes()` 后再调用 `stream()`，得到基于缓存的单块流 | 调用 `sse_messages()` 或 `sse_chunks()` 后继续读同一个响应 |
| 在同一条表达式中链式设置 `sse_*` 选项并消费 SSE | 对同一个 `HttpResponse` 同时设计多条消费路径 |

`retry_after_hint()` 会在响应状态为 429 或 5xx 且存在合法 `Retry-After` header 时返回延迟。它支持 `delta-seconds` 和 HTTP-date 两种格式；HTTP-date 早于当前时间时返回 0 秒。`HttpResponseMeta` 和 `HttpResponseInterceptorContext` 上也有同名方法，响应拦截器可以在只拿到 metadata 时读取这个提示。

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

| 分组 | 错误类型 | 典型含义 |
| --- | --- | --- |
| URL / 配置 | `InvalidUrl`, `BuildClient`, `ProxyConfig` | URL 无法解析、客户端构建失败或代理配置非法 |
| 超时 / 网络 | `ConnectTimeout`, `ReadTimeout`, `WriteTimeout`, `RequestTimeout`, `Transport` | 连接、读取、写入、整体请求超时，或底层传输失败 |
| HTTP 状态 | `Status` | 收到非 2xx 状态码 |
| 解码 / SSE | `Decode`, `SseProtocol`, `SseDecode` | 响应体解码失败、SSE 协议错误或 SSE JSON chunk 解码失败 |
| 重试层 | `RetryAttemptTimeout`, `RetryMaxElapsedExceeded`, `RetryAborted` | 重试执行器产生的 attempt timeout、总耗时耗尽或策略中止 |
| 取消 / 兜底 | `Cancelled`, `Other` | 请求取消，或无法归入其它分类的错误 |

`RetryAttemptTimeout` 表示单次重试尝试超过了重试层配置的 attempt timeout；`RetryMaxElapsedExceeded` 表示重试总耗时预算在尚未捕获可重试错误时已经耗尽；`RetryAborted` 表示 `qubit-retry` 决策器判定当前错误不可重试并提前中止，原始 `HttpError` 会作为 `source` 链接保留。

`retry_hint()` 会把超时、transport、429 和 5xx 状态视为可重试提示，其余默认不可重试。新增的重试层错误分类本身也不可重试。真正是否重试还要结合 `HttpRetryOptions` 和方法策略。

## 自动重试

自动重试默认关闭。开启后，只有当 `retry.enabled = true`、`max_attempts > 1`，并且方法策略允许当前 HTTP 方法时，`execute` 才会进入重试流程。

默认重试条件：

| 类型 | 默认可重试 |
| --- | --- |
| 状态码 | 429 和所有 5xx |
| 非状态错误 | connect/read/write/request timeout 和 transport |
| 方法 | GET、HEAD、PUT、DELETE、OPTIONS、TRACE |

可以用 `retry.status_codes` 和 `retry.error_kinds` 配置白名单。白名单一旦设置，就只重试列出的状态码或错误类型。

`retry.error_kinds` 可使用所有 `HttpErrorKind` 名称的配置形式，包括 `retry_attempt_timeout`、`retry_max_elapsed_exceeded`、`retry_aborted`。配置值会 trim，`read-timeout` 一类连字符形式会归一化为 `read_timeout`；请使用小写 snake_case 或等价的连字符形式。

对单个请求可以覆盖：

```rust
let request = client
    .request(Method::POST, "/submit")
    .force_retry()
    .retry_method_policy(qubit_http::HttpRetryMethodPolicy::AllMethods)
    .honor_retry_after(true)
    .build();
```

`honor_retry_after(true)` 只在请求级启用。遇到可重试的 429 或 5xx 时，如果响应里有 `Retry-After`，重试执行器会确保下一次尝试至少等待该 header 指定的时间；如果执行器计划的退避时间已经更长，则不会额外等待。

开启重试后，`execute` 会把每次尝试交给 `qubit-retry` 的 `Retry`。HTTP `max_duration` 会映射到 `qubit-retry` 的 `max_total_elapsed`，因此它使用单调时间统计，并包含 attempt 执行、retry 退避 sleep、`Retry-After` sleep 以及 retry 控制路径 listener 时间。可重试错误在耗尽 `max_attempts` 或 `max_duration` 后返回最后一次 HTTP 错误，并在 `message` 中追加耗尽原因；如果错误不满足当前重试白名单或方法策略，执行器会返回 `RetryAborted`，并把被中止的原始 `HttpError` 作为 `source` 保留。

| 场景 | 返回错误 | 说明 |
| --- | --- | --- |
| 方法策略不允许重放，例如默认策略下的 POST | 原始单次执行错误 | 不进入重试流程 |
| 已进入重试流程，但当前错误不可重试 | `RetryAborted` | 原始 `HttpError` 保存在 `source` 中 |
| 可重试，但耗尽 `max_attempts` | 最后一次 `HttpError` | `message` 会追加 attempts exhausted 上下文 |
| 可重试，但耗尽 `max_duration` | 最后一次 `HttpError` 或 `RetryMaxElapsedExceeded` | 已捕获过可重试错误时返回最后一次错误；尚未捕获时返回 `RetryMaxElapsedExceeded` |

如果需要从 `RetryAborted` 中读取原始状态码或错误分类，可以向下转型 `source`：

```rust
if error.kind == qubit_http::HttpErrorKind::RetryAborted {
    if let Some(source) = error.source.as_deref() {
        if let Some(inner) = source.downcast_ref::<qubit_http::HttpError>() {
            eprintln!("original kind={:?}, status={:?}", inner.kind, inner.status);
        }
    }
}
```

## 日志脱敏

HTTP 日志使用 `tracing::trace!`。必须同时满足：

1. `options.logging.enabled = true`。
2. 当前 tracing subscriber 开启 TRACE 级别。

可分别控制请求头、请求体、响应头、响应体。body 只记录前 `logging.body_size_limit` 字节，超出部分显示截断提示；二进制体显示为 `<binary N bytes>`。没有结构化或文本 `Content-Type` 的 unsupported body 会显示为 `<redacted: unsupported HTTP body>`，不会直接打印原始内容。请求体日志只预览已缓冲的 body 变体（`bytes_body`、`text_body`、`json_body`、`form_body`、`multipart_body`、`ndjson_body`）；`stream_body` 和 `streaming_body` 会记录为 `<skipped: streaming request body>`，因为 logger 不会消费上传流。

日志统一经过 `LogSanitizer` 和 `LogSanitizePolicy` 脱敏，底层复用 `qubit-sanitize` 的 URL、header 和 HTTP body 适配器。URL username、password、fragment 和敏感 query 参数会被掩码；JSON/form/multipart body 字段如果命中策略中的敏感名称，也会被掩码。具体 mask 字符串遵循 `qubit-sanitize` 的敏感级别：token/header 类字段通常显示为 `****`，`password`、`client_secret` 这类 secret 字段显示为 `<redacted>`。multipart 脱敏适用于所有 `multipart/*` 媒体类型。multipart 文件 part 会显示为 `<redacted: file part>`；格式异常、缺少 boundary 或已截断的 multipart body 会显示为 `<redacted: multipart body>`，避免原始上传字节泄露到日志。

默认敏感名称和掩码级别来自 `qubit_sanitize::SensitiveFields`，`rs-http` 不再维护或导出自己的敏感名称列表。匹配时会 trim、转小写、移除 `_`、`-`、`.`、空格等常见分隔符，并启用后缀匹配，因此 `access_token`、`access-token`、`accessToken` 和 `x-openai-api-key` 会命中同一类默认项。`log_sanitize.*` 下的配置项会扩展默认集合；代码里也可以直接调整 `options.log_sanitize_policy`，如果确实要使用完全自定义集合，可以先把对应字段赋值为 `qubit_sanitize::SensitiveFields::new()`。

示例：

```rust
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};
use qubit_sanitize::SensitivityLevel;
use serde_json::json;

let mut options = HttpClientOptions::new();
options.logging.enabled = true;
options.logging.log_request_header = true;
options.logging.log_request_body = true;
options
    .log_sanitize_policy
    .sensitive_headers
    .insert("x-api-key", SensitivityLevel::High);
options
    .log_sanitize_policy
    .sensitive_query_params
    .insert("access_token", SensitivityLevel::High);
options
    .log_sanitize_policy
    .sensitive_body_fields
    .insert("password", SensitivityLevel::Secret);

let client = HttpClientFactory::new().create(options)?;
let request = client
    .request(Method::POST, "https://api.example.com/login")
    .query_param("access_token", "secret-token")
    .header("x-api-key", "secret-key")?
    .json_body(&json!({
        "username": "alice",
        "password": "secret-password",
    }))?
    .build();

client.execute(request).await?;
```

注意：如果 TRACE 日志开启且 `log_response_body = true`，响应体日志只会在 body 已缓存，或响应不是 SSE 且存在 `Content-Length`、并且长度不超过 `logging.body_size_limit` 时读取并缓存完整 body；未知长度、超过限制或 SSE 响应会记录为跳过，不会为了日志消费后端流。

## 代理与 IPv4-only

代理默认禁用；同时 `use_env_proxy` 默认也是 `false`，因此客户端调用 `no_proxy()`，不会使用环境变量代理。若需要在未配置显式代理时继承 `HTTP_PROXY` / `HTTPS_PROXY` 等环境变量，可设置 `options.use_env_proxy = true` 或配置 `use_env_proxy = true`。启用显式代理时必须设置 host 和 port。

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

先按目标选择 API：

| 目标 | 使用 |
| --- | --- |
| 读取原始 SSE message | `response.sse_messages()` |
| 读取 OpenAI 风格 JSON chunk 或 `[DONE]` 完成标记 | `response.sse_chunks::<T>()` |
| 长连接断开后自动重连 | `client.execute_sse_with_reconnect(...)` |

SSE 消息解码从 `HttpResponse` 开始：

```rust
use futures_util::StreamExt;
use http::Method;

let response = client
    .execute(client.request(Method::GET, "/stream").build())
    .await?;

let mut messages = response.sse_messages();
while let Some(item) = messages.next().await {
    let message = item?;
    println!(
        "event={:?} last_event_id={:?} data={}",
        message.event, message.last_event_id, message.data
    );
}
```

### 配置 `sse_messages` 选项

`sse_max_line_bytes` 与 `sse_max_frame_bytes` 均返回 `HttpResponse`（按值移动后的 `self`），可在消费响应体之前与 `sse_messages()` 写在同一条链上，依次写入本响应上的 SSE 解析上限（`sse_messages` 会按此时的配置从 `stream()` 解码）：

```rust
use futures_util::StreamExt;
use http::Method;

let response = client
    .execute(client.request(Method::GET, "/stream").build())
    .await?;

let mut messages = response
    .sse_max_line_bytes(64 * 1024)      // 单行最大字节数
    .sse_max_frame_bytes(1024 * 1024) // 单帧最大字节数
    .sse_messages();
while let Some(item) = messages.next().await {
    let message = item?;
    println!(
        "event={:?} last_event_id={:?} data={}",
        message.event, message.last_event_id, message.data
    );
}
```

`SseMessage` 字段：

| 字段 | 含义 |
| --- | --- |
| `event` | `event:` 字段，可选 |
| `data` | 多行 `data:` 用 `\n` 拼接 |
| `last_event_id` | 应用当前帧以及之前 control-only `id:` 帧后的 EventSource last-event-id |

协议行为：

- 按 `\n` 分行，剥离行尾 `\r`。
- 每行必须是 UTF-8，否则返回 `HttpErrorKind::SseProtocol`。
- 空行在存在可派发 `data:` 时 flush 一个 message。
- 注释行（以 `:` 开头）忽略。
- 未知字段忽略。
- `retry:` 和 control-only `id:` 帧只作为内部控制记录使用，不会作为 `SseMessage` 暴露。
- 流结束时如果还有未 flush 的可派发字段，会输出最后一个 message。
- 单行和单帧上限默认来自 `HttpClientOptions`；若本次响应需要不同上限，在调用 `sse_messages` 之前将 `sse_max_line_bytes` / `sse_max_frame_bytes` 与 `sse_messages()` 链在同一条表达式上即可（完整示例见本节上文「配置 `sse_messages` 选项」）。

### SSE JSON chunk

以下代码片段假设已经构建好 `request`，并且 `MyChunk` 和 `handle` 已在调用方定义。

`sse_chunks` 无参数：完成标记策略默认为 `DoneMarkerPolicy::DefaultDone`（即 `DoneMarkerPolicy` 的 `Default` 实现，识别 trim 后等于 `[DONE]` 的 `data:`），并可通过 `HttpClientOptions::sse_done_marker_policy` 或响应上的 `sse_done_marker_policy` 覆盖。

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

### 配置 `sse_chunks` 选项

`sse_json_mode`、`sse_done_marker_policy`、`sse_max_line_bytes`、`sse_max_frame_bytes` 均返回 `HttpResponse`（按值移动后的 `self`），可在调用 `sse_chunks::<T>()` 之前与之一同链式覆盖本响应上的 SSE JSON 模式、完成标记策略与行/帧上限：

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

`DoneMarkerPolicy` 支持：

- `Disabled`：不识别完成标记。
- `DefaultDone`：`data:` 去除空白后等于 `[DONE]` 时输出 `SseChunk::Done` 并结束。
- `Custom(String)`：使用自定义完成标记。

`SseJsonMode::Lenient` 会跳过 malformed JSON chunk 并继续；`Strict` 会在第一个 malformed JSON 处返回 `HttpErrorKind::SseDecode`。默认的 JSON 模式、完成标记策略与行/帧上限可在 `HttpClientOptions`（及配置里的 `sse.*`）中设置；若仅本次响应需要不同取值，按上文「配置 `sse_chunks` 选项」小节将 `sse_json_mode`、`sse_done_marker_policy`、`sse_max_line_bytes`、`sse_max_frame_bytes` 与 `sse_chunks::<T>()` 链在同一条表达式上即可。

### SSE 自动重连

```rust
use futures_util::StreamExt;
use http::Method;
use qubit_http::sse::SseReconnectOptions;
use qubit_http::{RetryDelay, RetryJitter, RetryOptions};

let request = client.request(Method::GET, "/events").build();
let mut events = client.execute_sse_with_reconnect(
    request,
    SseReconnectOptions {
        retry: RetryOptions::new(
            6, // max_attempts = 1 次初始连接 + 5 次重连
            None,
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
        server_retry_max_delay: None,
        apply_jitter_to_server_retry: true,
    },
);

while let Some(item) = events.next().await {
    let event = item?;
    println!("{}", event.data);
}
```

默认重连配置为 `retry.max_attempts = 4`（即最多重连 3 次），延迟策略为 `RetryDelay::Exponential { initial=1s, max=30s, multiplier=2.0 }`，jitter 为 `RetryJitter::None`，并在 EOF 后重连、尊重服务端 `retry:`。服务端 `retry:` 会被限制到 `server_retry_max_delay`；若该字段为 `None`，则从重试延迟策略的最大值推导，无法推导时使用 30 秒。`apply_jitter_to_server_retry` 默认开启，但默认重连 jitter 是 `RetryJitter::None`，因此除非调用方提供带 jitter 的 `RetryOptions`，服务端给出的延迟不会被扰动。重连时会复用原始请求，并禁用内层 HTTP 自动重试，避免 HTTP 重试和 SSE 重连相乘；如果之前收到过 SSE `id:`，下一次请求会带上 `Last-Event-ID` header。`execute_sse_with_reconnect` 会要求响应 `Content-Type` 为 `text/event-stream`。取消错误不会触发重连；SSE 协议错误默认也不会重连；超时、transport、429/5xx 等 retryable 错误，以及包含 unexpected EOF 语义的错误可触发重连。

## 配置参考

下表列出 `HttpClientOptions::from_config` 支持的完整配置键。若传入 `config.prefix_view("http")`，这些键都按相对路径读取。

| 键 | 说明 |
| --- | --- |
| `base_url` | 相对请求路径的基础 URL |
| `ipv4_only` | 启用后 DNS 只保留 IPv4 地址，并拒绝 IPv6 literal URL |
| `error_response_preview_limit` | 非 2xx 错误中保留的响应体预览字节数 |
| `user_agent` | 传给 `reqwest` builder 的默认 User-Agent |
| `max_redirects` | 最大重定向次数 |
| `pool_idle_timeout` | 连接池空闲超时 |
| `pool_max_idle_per_host` | 每个 host 最大空闲连接数 |
| `use_env_proxy` | 显式代理禁用时是否继承环境代理；默认 `false` |
| `log_sanitize.sensitive_headers` | 追加到默认敏感 header 集合的字符串列表 |
| `log_sanitize.sensitive_query_params` | 追加到默认敏感 query 参数集合的字符串列表 |
| `log_sanitize.sensitive_body_fields` | 追加到默认敏感 body 字段集合的字符串列表 |
| `default_headers` | 默认请求 header 的 JSON map 字符串；不能与 `default_headers.<name>` 同时使用 |
| `default_headers.<name>` | 一个默认请求 header 子键；不能与 `default_headers` JSON map 同时使用 |
| `timeouts.connect_timeout` | 连接超时 |
| `timeouts.read_timeout` | 读取响应体或流时的单次等待超时 |
| `timeouts.write_timeout` | 发送前准备和发送阶段超时 |
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
| `sse.done_marker` | `DISABLED`（或 `DISABLE`）禁用完成标记；`DEFAULT` 使用 `[DONE]`；其它非空字符串视为自定义完成标记（`Custom`），与 trim 后的 `data:` 文本比较 |
| `sse.max_line_bytes` | SSE 单行字节上限 |
| `sse.max_frame_bytes` | SSE 单帧字节上限 |

## 实用建议

- 对只读或幂等接口开启全局重试；对 POST/PATCH 只有在业务允许重放时才使用 `AllMethods` 或请求级强制重试。
- 对长连接/SSE 设置合理的 `read_timeout`；过短会把正常的慢流误判为 `ReadTimeout`。
- TRACE 响应体日志只会预读并缓存已知长度且不超过日志限制的非 SSE body；真正的未知长度流式响应和 SSE 不会为了日志被消费。
- 需要完全禁用代理时保持默认 `proxy.enabled = false` 且 `use_env_proxy = false`；需要继承环境代理时显式打开 `use_env_proxy`。
- 如果使用 `from_config`，优先传入 `prefix_view("http")` 一类的作用域视图，这样错误路径会保留完整上下文。
