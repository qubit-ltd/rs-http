# Qubit HTTP（`rs-http`）

[![CircleCI](https://circleci.com/gh/qubit-ltd/rs-http.svg?style=shield)](https://circleci.com/gh/qubit-ltd/rs-http)
[![Coverage Status](https://coveralls.io/repos/github/qubit-ltd/rs-http/badge.svg?branch=main)](https://coveralls.io/github/qubit-ltd/rs-http?branch=main)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
[![Rust](https://img.shields.io/badge/rust-1.70+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

`qubit-http` 是一个 Rust HTTP 基础设施库，用于构建行为一致的 API 客户端，核心能力包括：

- 用统一的客户端模型处理普通请求和流式请求
- 显式配置超时、重试、代理、日志
- 支持请求级重试覆盖与取消能力（`CancellationToken`）
- 支持同步/异步 Header Injector 链并保证优先级顺序
- 内置 JSON / form / multipart / NDJSON 请求体构建
- 内置 SSE 事件解析和 JSON chunk 解码
- 统一错误模型（`HttpError`、`HttpErrorKind`、`RetryHint`）

如果你是第一次接触本库，建议先看 **快速开始**，再跳到对应场景示例。

## 安装

```toml
[dependencies]
qubit-http = "0.2.0"
http = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
futures-util = "0.3"
qubit-config = "0.8" # 可选：仅在 create_from_config(...) 示例中需要
```

## 快速开始

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

## 核心类型

- `HttpClientFactory`：通过默认配置、显式配置或配置中心创建客户端。
- `HttpClientOptions`：管理 `base_url`、默认 Header、超时、代理、重试、日志、敏感头和 SSE 解码默认项。
- `HttpClient`：执行请求，提供一致的运行时行为，支持同步/异步 Header Injector。
- `HttpRequestBuilder`：构建路径、查询参数、Header、Body、请求级超时、请求级重试覆盖和取消 token。
- `HttpResponse`：完整缓冲响应（`status`、`headers`、`body`、`text()`、`json()`）。
- `HttpStreamResponse`：响应元数据 + 字节流（`into_stream()`），支持可配置 SSE 行/帧大小限制。
- `qubit_http::sse`：SSE 事件解析和 JSON chunk 解码工具。

## 为什么使用本库（对比 `http` 与 `reqwest`）

先说明：Rust 标准库本身不提供 HTTP 客户端。通常：
- `http` crate：只提供 HTTP 类型（`Request`/`Response`/`Method`/`Header`）
- `reqwest`：通用 HTTP 客户端
- `qubit-http`：基于 `reqwest` 的团队级 HTTP 基础设施层

| 维度 | `http` crate | `reqwest` | `qubit-http` |
| --- | --- | --- | --- |
| 核心定位 | 类型定义 | 通用 HTTP 客户端 | 统一 HTTP 基础设施层 |
| 是否发请求 | 否 | 是 | 是 |
| 重试/超时模型 | 无 | 基础能力 | 统一默认策略 + 请求级覆盖 |
| 错误语义 | 无 | 以 `reqwest::Error` 为主 | `HttpError` + `HttpErrorKind` + `RetryHint` |
| 配置一致性 | 无 | 业务自行组织 | `HttpClientOptions` + 工厂 + 校验 |
| 流式/SSE | 无 | 原始字节流 | SSE 事件/JSON 解码 + 安全限制 |
| 可观测性与安全 | 无 | 业务自行处理 | 脱敏、限长预览、统一日志行为 |
| 跨服务一致性 | 低 | 中 | 高 |

当你希望“多服务保持一致行为”，而不是“每个项目重复造一套 HTTP 规范”时，优先使用 `qubit-http`。

## 常见用法

### 1）构建带 query/header/body 的请求

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

### 2）Header 注入与优先级

Header 合并优先级为：

`默认 headers` -> `同步 header injector` -> `异步 header injector` -> `请求级 headers（最高优先级）`

```rust
use http::HeaderValue;
use qubit_http::{AsyncHeaderInjector, HeaderInjector, HttpClientFactory};

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
    client.add_async_header_injector(AsyncHeaderInjector::new(|headers| {
        Box::pin(async move {
            headers.insert(
                "x-auth-source",
                HeaderValue::from_static("async-refresh"),
            );
            Ok(())
        })
    }));

    Ok(client)
}
```

### 3）超时与重试

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

触发自动重试需要同时满足：

- `retry.enabled = true`
- `max_attempts > 1`
- HTTP 方法被 `method_policy` 允许
- 错误属于可重试类型（`timeout`、`transport`、`429`、`5xx`）

### 4）请求级重试覆盖（force/disable/method/Retry-After）

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

### 5）请求取消（`CancellationToken`）

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
    let _ = client.execute_stream(request).await?;
    Ok(())
}
```

### 6）form / multipart / NDJSON 请求体

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

### 7）消费流式字节响应

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

### 8）解码 SSE JSON chunk

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

你可以通过 `HttpClientOptions` 配置客户端级 SSE 默认值：

```rust
use qubit_http::sse::SseJsonMode;

options.sse_json_mode = SseJsonMode::Strict;
options.sse_max_line_bytes = 64 * 1024;
options.sse_max_frame_bytes = 1024 * 1024;
```

你仍可按请求覆盖：使用 `response.decode_json_chunks_with_mode(...)`
或 `decode_json_chunks_with_mode_and_limits(...)`：

```rust
use qubit_http::sse::{DoneMarkerPolicy, SseJsonMode};

let chunks = response.decode_json_chunks_with_mode_and_limits::<MyChunk>(
    DoneMarkerPolicy::DefaultDone,
    SseJsonMode::Lenient,
    64 * 1024,   // max_line_bytes
    1024 * 1024, // max_frame_bytes
);
```

### 9）OpenAI SSE 简化示例（`chat.completions` 与 `responses`）

下面只展示“请求 + 解码”主流程。
OpenAI 的字段细节以最新 API 文档为准：

- Chat Completions 流式事件：
  `https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events`
- Responses 流式事件：
  `https://developers.openai.com/api/reference/resources/responses/methods/create`

```rust
use futures_util::StreamExt;
use http::Method;
use qubit_http::sse::{DoneMarkerPolicy, SseChunk};
use serde_json::json;

// 请按 OpenAI 官方文档在你的项目中定义这些结构体：
// - ChatCompletionChunk：
//   https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events
// - ResponseOutputTextDeltaEvent：
//   https://developers.openai.com/api/reference/resources/responses/methods/create

/// 解析 OpenAI Chat Completions 流：
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
            "messages": [{"role": "user", "content": "请用一句话打招呼"}],
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

/// 解析 OpenAI Responses 流：
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
            "input": "给一条 Rust 小技巧",
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

### 10）从配置创建客户端

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
    config.set("http.sse.json_mode", "STRICT".to_string()).unwrap();
    config.set("http.sse.max_line_bytes", 64 * 1024usize).unwrap();
    config
        .set("http.sse.max_frame_bytes", 1024 * 1024usize)
        .unwrap();

    HttpClientFactory::new().create_from_config(&config.prefix_view("http"))
}
```

## 错误处理

`execute(...)` 和 `execute_stream(...)` 会在以下场景返回 `Err(HttpError)`：

- URL 解析失败
- 传输错误或超时
- HTTP 状态码非 2xx（`HttpErrorKind::Status`）
- 响应解码或 SSE 解码失败
- 请求被取消（`HttpErrorKind::Cancelled`）

可以通过 `error.kind` 和 `error.retry_hint()` 分流处理：

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

## 默认值

| 配置项 | 默认值 |
| --- | --- |
| `connect_timeout` | `10s` |
| `read_timeout` | `120s` |
| `write_timeout` | `120s` |
| `request_timeout` | `None` |
| `proxy.enabled` | `false` |
| `proxy.proxy_type` | `http` |
| `logging.enabled` | `true` |
| `logging.*` 头/体日志开关 | 全部 `true` |
| `logging.body_size_limit` | `16 * 1024` 字节 |
| `retry.enabled` | `false` |
| `retry.max_attempts` | `3` |
| `retry.delay_strategy` | 指数退避（`200ms`、`5s`、`2.0`） |
| `retry.jitter_factor` | `0.1` |
| `retry.method_policy` | `IdempotentOnly` |
| `ipv4_only` | `false` |
| `sse.json_mode` | `Lenient` |
| `sse.max_line_bytes` | `64 * 1024` 字节 |
| `sse.max_frame_bytes` | `1024 * 1024` 字节 |

## 说明

- 本库聚焦稳定、通用的 HTTP 抽象，不覆盖 `reqwest` 的全部 API。
- 日志基于 `tracing`，需要启用 `TRACE` 级别订阅器才能看到完整输出。
- 敏感 Header 会自动脱敏。
- 二进制或非 UTF-8 的 Body 不会按原文打印，而是输出摘要。
- `proxy.enabled = false` 时，不继承环境变量代理。
- `ipv4_only` 会强制 DNS 仅使用 IPv4 地址，并拒绝 IPv6 字面量目标/代理主机。
- `create_with_options(...)` 总会执行 `HttpClientOptions::validate()`，非法配置会快速失败。
- 流式重试仅覆盖返回 `HttpStreamResponse` 之前的失败；流开始后的错误由调用方处理。

## 许可证

Apache 2.0
