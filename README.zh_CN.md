# Qubit HTTP（`rs-http`）

[![Rust CI](https://github.com/qubit-ltd/rs-http/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-http/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-http/coverage-badge.json)](https://qubit-ltd.github.io/rs-http/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
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
qubit-http = "0.12"
qubit-redact = "0.5"
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

所有 TRACE 与 `Debug` 路径共享一份不可变的根 `RedactionPolicy` 快照。
其 `http()` 视图只保存 HTTP 上下文差异；基础字段规则、掩码和静态限制与其他 adapter
共享。规范 `qubit_redact::formats::http::HttpRedactor` 统一处理 URL 用户信息、fragment、query 字段、
原生敏感 header、结构化 body 和硬预算。非根 URL path、不透明文本和无键 JSON 值默认隐藏。

`RedactionPolicy::builder()` 使用空应用规则和标准 floor。
扩展保守默认快照时使用 `RedactionPolicy::default().to_builder()`；
只有显式调用 `.disable_all_floors()` 才会关闭全部 floor 保护：

应用启动时使用 `Redactor::set_default()` 安装默认 redactor；`RedactionPolicy::default()` 始终
返回固定标准策略。`HttpClientOptions::new()` 会取得构造时默认 redactor 的快照，包括此前已安装的应用策略。既有 client、request、
response 和 error 会保留原来的 redactor；每个脱敏操作默认使用策略中的诊断预算。如果一个记录需要渲染多个字段，
可创建 `let mut session = redactor.session()`，再通过 `session.http(...)` 处理 HTTP 字段，以共享同一个运行时预算。

```rust
use qubit_http::{HttpClientFactory, HttpClientOptions};
use qubit_redact::{RedactionPolicy, Sensitivity};
use qubit_redact::formats::http::UrlPathPolicy;

let mut options = HttpClientOptions::new();
let mut builder = RedactionPolicy::default().to_builder();
builder.http().header().raise("x-api-key", Sensitivity::High)?;
builder.http().query().raise("access_token", Sensitivity::High)?;
builder.http().body().raise("password", Sensitivity::Secret)?;
builder.http().query().allow_exact("known_public_token")?;
builder.http().url_path(UrlPathPolicy::Preserve);
options.log_redaction_policy = builder.build()?;

let client = HttpClientFactory::new().create(options)?;
```

`logging.body_size_limit` 是展示限额；policy 中的 `BodyBudget` 是第二层不可绕过的输入与
输出硬上限。截断 body 统一使用 `<truncated>` 标记；调用方知道源长度时，结果保留精确
源长度元数据。配置只读取 `log_redaction` section，不兼容旧 key。只有在应用明确接受
移除 HTTP 上下文 floor 时，才使用 `builder.http().disable_all_floors()`。

`HttpError` 的 `Debug` 与 `Display` 都会应用同一套日志脱敏策略，因此常规错误格式化不会
暴露 URL 中的敏感值。

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
| `HttpResponseInterceptorContext` | 让响应拦截器检查 status/method，并修改 headers/最终 URL，同时不破坏成功状态不变量。 |

## 项目范围

- `qubit-http` 基于 `reqwest` 构建，重点是提供稳定、统一的 HTTP 基础设施层，而不是暴露 `reqwest` 的全部 API。
- 响应体默认惰性读取；只有开启 TRACE 级响应体日志时才会提前读取。
- 内置请求重试只覆盖返回 `HttpResponse` 之前的失败。返回后的流式响应体错误会交给调用方处理。
- SSE 重连使用独立 API：`HttpClient::execute_sse_with_reconnect(...)`。

## 测试

```bash
# 使用默认 feature 集运行测试
cargo test

# 使用项目声明的全部 feature 运行测试
cargo test --all-features

# 运行项目 CI 检查
./ci-check.sh

# 检查代码覆盖率
./coverage.sh
```

## 许可证

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

本项目基于 Apache License 2.0 授权。完整许可证文本请参阅
[LICENSE](LICENSE)。

## 贡献

欢迎贡献。请遵循 Rust API 指南，及时更新公共 API 文档与测试，并在提交
Pull Request 前运行 `./align-ci.sh`格式化代码，运行`./ci-check.sh`对齐CI要求。

## 作者

**Haixing Hu** - *Qubit Co. Ltd.*

仓库地址：[https://github.com/qubit-ltd/rs-http](https://github.com/qubit-ltd/rs-http)
