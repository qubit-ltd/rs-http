# Qubit HTTP（`rust-http`）

[![CircleCI](https://circleci.com/gh/qubit-ltd/rs-http.svg?style=shield)](https://circleci.com/gh/qubit-ltd/rs-http)
[![Coverage Status](https://coveralls.io/repos/github/qubit-ltd/rs-http/badge.svg?branch=main)](https://coveralls.io/github/qubit-ltd/rs-http?branch=main)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
[![Rust](https://img.shields.io/badge/rust-1.70+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

一个通用的 Rust HTTP 基础设施 crate，提供统一客户端语义、安全日志，以及内置 SSE 解码能力。

## 功能特性

- 统一 HTTP 选项：
  - `base_url`、`default_headers`、timeouts、proxy、logging、sensitive headers、`ipv4_only`
- 工厂封装：
  - `HttpClientFactory`（基于 reqwest）
  - `HttpClientFactory::create()` 使用默认选项创建客户端
  - `HttpClientFactory::create_with_options(...)` 使用显式选项创建客户端
  - `HttpClientFactory::create_from_config(...)` 从配置创建客户端
- 高频客户端 API：
  - `request(...)`、`execute(...)`、`execute_stream(...)`
- Header 便捷方法：
  - `HttpClientOptions::add_header(s)` 用于创建前配置默认头
  - `HttpClient::add_header(s)` 用于创建后配置默认头
- Header 注入链路：
  - `默认 headers -> 注入器 -> 请求级 headers（最终覆盖）`
- 超时语义：
  - `connect_timeout` 由 reqwest 处理
  - `write_timeout` 包装发送阶段
  - `read_timeout` 包装 body/chunk 读取阶段
  - 可选 `request_timeout` 作为 reqwest 总超时
- 代理能力：
  - `http` / `https` / `socks5`
  - 支持代理认证
  - 显式 `proxy.enabled = false` 时不继承环境代理
- 日志与脱敏：
  - 请求/响应 header/body 独立开关
  - 敏感头脱敏规则：`<=4 => ****`，否则保留前 2 后 2
  - 二进制或非 UTF-8 body 不输出原文
- 内置 SSE 解码（`qubit_http::sse`）：
  - 行解码（`\n` / `\r\n`）
  - 事件分帧（`data/event/id/retry`）
  - 完结标记策略（`[DONE]` / 自定义 / 关闭）
  - JSON chunk 解码（宽松/严格模式）
- 统一错误模型：
  - `HttpError`、`HttpErrorKind`、`RetryHint`

## 安装

```toml
[dependencies]
qubit-http = "0.2.0"
```

## 使用场景示例

### 1）基础请求（JSON 响应）

```rust
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = HttpClientOptions::default();
    options.set_base_url("https://example.com")?;
    options.add_header("x-client-id", "demo")?;

    let client = HttpClientFactory::new().create_with_options(options)?;

    let request = client.request(Method::GET, "/health").build();
    let response = client.execute(request).await?;

    println!("status={}", response.status);
    println!("body={}", response.text()?);
    Ok(())
}
```

### 2）构建请求（query/header/body）

```rust
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};

#[derive(serde::Serialize)]
struct CreateMessageRequest {
    role: String,
    content: String,
}

async fn create_message() -> qubit_http::HttpResult<()> {
    let mut options = HttpClientOptions::default();
    options.set_base_url("https://api.example.com")?;
    options.add_headers([
        ("x-client-id", "demo"),
        ("x-env", "test"),
    ])?;
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

### 3）Header 注入器（认证头/租户头）

```rust
use http::HeaderValue;
use qubit_http::{HeaderInjector, HttpClientFactory};

fn build_client_with_injector() -> qubit_http::HttpResult<qubit_http::HttpClient> {
    let token = "secret-token".to_string();
    let mut client = HttpClientFactory::new().create()?;
    client.add_header_injector(HeaderInjector::new(move |headers| {
        let value = HeaderValue::from_str(&format!("Bearer {}", token))
            .map_err(|e| qubit_http::HttpError::other(format!("invalid auth header: {e}")))?;
        headers.insert(http::header::AUTHORIZATION, value);
        headers.insert("x-tenant-id", HeaderValue::from_static("tenant-a"));
        Ok(())
    }));
    Ok(client)
}
```

### 4）超时配置（全局 + 请求级覆盖）

```rust
use std::time::Duration;

use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};

async fn request_with_timeouts() -> qubit_http::HttpResult<()> {
    let mut options = HttpClientOptions::default();
    options.timeouts.connect_timeout = Duration::from_secs(3);
    options.timeouts.read_timeout = Duration::from_secs(30);
    options.timeouts.write_timeout = Duration::from_secs(15);
    options.timeouts.request_timeout = Some(Duration::from_secs(60));

    let client = HttpClientFactory::new().create_with_options(options)?;

    // 请求级 timeout 会覆盖 client 默认 request_timeout。
    let request = client
        .request(Method::GET, "https://example.com/slow")
        .timeout(Duration::from_secs(5))
        .build();
    let _ = client.execute(request).await?;
    Ok(())
}
```

### 5）代理（HTTP / HTTPS / SOCKS5）

```rust
use qubit_http::{HttpClientFactory, HttpClientOptions, ProxyType};

fn build_client_with_proxy() -> qubit_http::HttpResult<qubit_http::HttpClient> {
    let mut options = HttpClientOptions::default();
    options.proxy.enabled = true;
    options.proxy.proxy_type = ProxyType::Socks5; // 也可以是 ProxyType::Http / ProxyType::Https
    options.proxy.host = Some("127.0.0.1".to_string());
    options.proxy.port = Some(1080);
    options.proxy.username = Some("user".to_string());
    options.proxy.password = Some("pass".to_string());

    HttpClientFactory::new().create_with_options(options)
}
```

### 6）从配置创建客户端

```rust
use std::time::Duration;

use qubit_config::Config;
use qubit_http::HttpClientFactory;

fn build_client_from_config() -> Result<qubit_http::HttpClient, qubit_http::HttpConfigError> {
    let mut config = Config::new();
    config.set("http.base_url", "https://api.example.com".to_string()).unwrap();
    config.set("http.timeouts.connect_timeout", Duration::from_secs(3)).unwrap();
    config.set("http.proxy.enabled", false).unwrap();
    config.set("http.logging.enabled", true).unwrap();

    HttpClientFactory::new().create_from_config(&config, "http")
}
```

### 7）原始字节流消费

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

### 8）流式 + SSE JSON chunk（宽松模式）

```rust
use futures_util::StreamExt;
use http::Method;
use qubit_http::sse::{decode_json_chunks, DoneMarkerPolicy, SseChunk};

#[derive(Debug, serde::Deserialize)]
struct StreamChunk {
    delta: String,
}

async fn run_stream(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let request = client.request(Method::GET, "/v1/stream").build();
    let stream_response = client.execute_stream(request).await?;

    let mut chunks = decode_json_chunks::<StreamChunk>(
        stream_response,
        DoneMarkerPolicy::DefaultDone,
    );

    while let Some(item) = chunks.next().await {
        match item? {
            SseChunk::Data(chunk) => {
                println!("delta={}", chunk.delta);
            }
            SseChunk::Done => break,
        }
    }

    Ok(())
}
```

### 9）SSE 严格模式（坏 JSON 立即失败）

```rust
use futures_util::StreamExt;
use qubit_http::sse::{decode_json_chunks_with_mode, DoneMarkerPolicy, SseJsonMode};

#[derive(Debug, serde::Deserialize)]
struct Chunk {
    token: String,
}

async fn strict_sse(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let response = client
        .execute_stream(client.request(http::Method::GET, "/v1/stream").build())
        .await?;

    let mut stream = decode_json_chunks_with_mode::<Chunk>(
        response,
        DoneMarkerPolicy::DefaultDone,
        SseJsonMode::Strict,
    );

    while let Some(item) = stream.next().await {
        let _ = item?; // 坏 JSON 会返回 HttpErrorKind::SseDecode
    }
    Ok(())
}
```

### 10）错误分类与重试衔接

```rust
use qubit_http::{HttpErrorKind, RetryHint};

fn handle_http_error(error: &qubit_http::HttpError) {
    match error.kind {
        HttpErrorKind::Status if error.status.is_some() => {
            eprintln!("status error: {:?}", error.status);
        }
        HttpErrorKind::ReadTimeout | HttpErrorKind::WriteTimeout | HttpErrorKind::ConnectTimeout => {
            eprintln!("timeout: {}", error);
        }
        _ => {
            eprintln!("http error: {}", error);
        }
    }

    let should_retry = matches!(error.retry_hint(), RetryHint::Retryable);
    eprintln!("retryable={should_retry}");
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
| `logging.*` 头体开关 | 全部 `true` |
| `logging.body_size_limit` | `16 * 1024` 字节 |
| `ipv4_only` | `false` |

## 测试覆盖

测试位于 [`tests/`](tests)，覆盖：

- options 默认值与归一化
- 敏感头脱敏行为
- request builder 校验与 body 编码
- factory/代理配置校验
- 代理集成路径（`http` / `https CONNECT` / `socks5`）
- client `execute/execute_stream` 核心路径
- 状态码映射与超时行为
- logging 策略行为（开关/脱敏/二进制/截断）
- SSE 事件分帧与 JSON 解码行为

## 当前限制

- 本 crate 有意不封装 `reqwest` 全量 API。
- 非 HTTP 流式协议（WebSocket、gRPC）不在范围内。
- `ipv4_only` 当前是可配置并可校验的选项，传输层强制能力将在后续增强版本完成。

## 许可证

Apache 2.0
