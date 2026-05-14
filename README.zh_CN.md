# Qubit HTTP（`rs-http`）

[![Rust CI](https://github.com/qubit-ltd/rs-http/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-http/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-http/coverage-badge.json)](https://qubit-ltd.github.io/rs-http/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
[![Docs.rs](https://docs.rs/qubit-http/badge.svg)](https://docs.rs/qubit-http)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

文档：[用户指南](doc/user_guide.zh_CN.md) | [API 文档](https://docs.rs/qubit-http)

`qubit-http` 是一个面向生产使用的 Rust HTTP 基础设施库，用于构建跨服务行为一致的 API 客户端。

它基于 `reqwest` 构建，封装了 API 客户端最常见的基础能力：请求构建、超时、重试、取消、流式响应、SSE、日志和统一错误处理。

## 为什么使用

当你需要以下能力时，可以使用 `qubit-http`：

- 用统一流程执行普通响应、惰性响应体和流式响应
- 统一超时、重试、取消、代理、重定向和日志行为
- 通过 `HttpError`、`HttpErrorKind`、`RetryHint` 做一致的错误处理
- 内置 JSON、表单、multipart、NDJSON、流式请求和 SSE 辅助能力
- 通过配置创建客户端，让服务间行为保持一致

完整示例和高级选项请阅读[用户指南](doc/user_guide.zh_CN.md)。

## 安装

```toml
[dependencies]
qubit-http = "0.7"
http = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

用户指南中的部分示例会用到 `serde`、`serde_json`、`futures-util`、`qubit-config` 等可选辅助依赖。

## 快速开始

这个示例使用 `httpbin.org`，不需要额外启动本地测试服务。

```rust
use http::Method;
use qubit_http::{HttpClientFactory, HttpClientOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = HttpClientOptions::new();
    options.set_base_url("https://httpbin.org")?;
    options.add_header("x-client-id", "demo")?;

    let client = HttpClientFactory::new().create(options)?;

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

## 日志脱敏

HTTP TRACE 日志在输出前会统一脱敏。默认策略会掩码常见凭证类 header、
query 参数和 JSON/form body 字段。若某个服务使用自定义敏感名称，可以在
客户端配置中扩展脱敏策略：

```rust
use qubit_http::{HttpClientFactory, HttpClientOptions};

let mut options = HttpClientOptions::new();
options.logging.enabled = true;
options.logging.log_request_body = true;
options.log_sanitize_policy.sensitive_headers.insert("x-api-key");
options
    .log_sanitize_policy
    .sensitive_query_params
    .insert("access_token");
options
    .log_sanitize_policy
    .sensitive_body_fields
    .insert("password");

let client = HttpClientFactory::new().create(options)?;
```

## 后续阅读

| 任务 | 阅读 |
| --- | --- |
| 通过默认配置、代码配置或配置中心创建客户端 | [用户指南](doc/user_guide.zh_CN.md) |
| 构建 JSON、表单、multipart、NDJSON 或流式请求体 | [用户指南](doc/user_guide.zh_CN.md) |
| 添加默认请求头、请求头注入器和拦截器 | [用户指南](doc/user_guide.zh_CN.md) |
| 配置超时、重试、取消和 `Retry-After` 处理 | [用户指南](doc/user_guide.zh_CN.md) |
| 读取 bytes、text、JSON、流式响应或 SSE 数据块 | [用户指南](doc/user_guide.zh_CN.md) |
| 配置日志脱敏、代理、重定向和 IPv4-only 模式 | [用户指南](doc/user_guide.zh_CN.md) |
| 处理状态码、传输、超时、取消、解码和重试错误 | [用户指南](doc/user_guide.zh_CN.md) |

## 核心 API 概览

| 类型 | 用途 |
| --- | --- |
| `HttpClientFactory` | 通过默认配置、显式配置或配置中心创建客户端。 |
| `HttpClientOptions` | 保存客户端级默认配置，包括 base URL、请求头、超时、重试、日志、代理、重定向、连接池和 SSE 解码。 |
| `HttpClient` | 执行请求，并应用请求头、注入器、拦截器、重试、日志和 SSE 重连辅助能力。 |
| `HttpRequestBuilder` | 构建方法、路径、查询参数、请求头、请求体和请求级覆盖项。 |
| `HttpResponse` | 提供响应元数据，以及 bytes、text、JSON、流式响应和 SSE 的惰性读取方法。 |

## 项目范围

- `qubit-http` 基于 `reqwest` 构建，重点是提供稳定、统一的 HTTP 基础设施层，而不是暴露 `reqwest` 的全部 API。
- 响应体默认惰性读取；只有开启 TRACE 级响应体日志时才会提前读取。
- 内置请求重试只覆盖返回 `HttpResponse` 之前的失败。返回后的流式响应体错误会交给调用方处理。
- SSE 重连使用独立 API：`HttpClient::execute_sse_with_reconnect(...)`。

## 贡献

欢迎提交 issue 和 pull request。

为了让维护和评审更顺畅，请尽量遵循以下约定：

- bug 报告、设计问题或较大的功能建议，先提交 issue 讨论
- pull request 尽量聚焦一个行为变更、问题修复或文档更新
- 遵循 [Rust 编码风格指南](RUST_CODING_STYLE.zh_CN.md)
- 修改运行时行为时，请补充相应测试
- 公共 API 行为变化时，请同步更新用户指南或 README

向本项目提交贡献，即表示你同意该贡献使用与本项目相同的许可证。

## 许可证

本项目使用 [Apache License, Version 2.0](LICENSE) 许可证。
