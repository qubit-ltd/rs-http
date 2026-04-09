# Qubit HTTP（`rust-http`）

[![CircleCI](https://circleci.com/gh/qubit-ltd/rust-http.svg?style=shield)](https://circleci.com/gh/qubit-ltd/rust-http)
[![Coverage Status](https://coveralls.io/repos/github/qubit-ltd/rust-http/badge.svg?branch=main)](https://coveralls.io/github/qubit-ltd/rust-http?branch=main)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
[![Rust](https://img.shields.io/badge/rust-1.70+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

一个通用的 Rust HTTP 基础设施 crate，提供统一客户端语义、安全日志，以及内置 SSE 解码能力。

## 当前状态

`qubit-http` 已实现并具备测试覆盖。

- 已实现：选项、工厂、请求/响应/流式 API、日志脱敏、统一错误与重试提示、SSE 解码。
- 待增强：传输层 `IPv4-only` resolver 强制能力。

设计文档：

- PRD：[`doc/http_prd.zh_CN.md`](doc/http_prd.zh_CN.md)
- 实现方案：[`doc/http_design.zh_CN.md`](doc/http_design.zh_CN.md)

## 功能特性

- 统一 HTTP 选项：
  - `base_url`、`default_headers`、timeouts、proxy、logging、sensitive headers、`ipv4_only`
- 工厂封装：
  - `HttpClientFactory`（基于 reqwest）
- 高频客户端 API：
  - `request(...)`、`execute(...)`、`execute_stream(...)`
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
qubit-http = "0.1.0"
```

## 快速开始

```rust
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};
use url::Url;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = HttpClientOptions::default();
    options.base_url = Some(Url::parse("https://example.com")?);

    let client = HttpClientFactory::new().create(options)?;

    let request = client.request(Method::GET, "/health").build();
    let response = client.execute(request).await?;

    println!("status={}", response.status);
    println!("body={}", response.text()?);
    Ok(())
}
```

## 流式 + SSE 示例

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

## 与重试模块衔接

```rust
use qubit_http::RetryHint;

fn should_retry(error: &qubit_http::HttpError) -> bool {
    matches!(error.retry_hint(), RetryHint::Retryable)
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
- client `execute/execute_stream` 核心路径
- 状态码映射与超时行为
- SSE 事件分帧与 JSON 解码行为

## 当前限制

- 本 crate 有意不封装 `reqwest` 全量 API。
- 非 HTTP 流式协议（WebSocket、gRPC）不在范围内。
- `ipv4_only` 当前是可配置并可校验的选项，传输层强制能力将在后续增强版本完成。

## 许可证

Apache 2.0
