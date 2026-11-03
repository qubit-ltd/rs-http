# Qubit HTTP（`rs-http`）

[![Rust CI](https://github.com/qubit-ltd/rs-http/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-http/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-http/coverage-badge.json)](https://qubit-ltd.github.io/rs-http/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

`qubit-http` 是一个面向生产使用的 Rust HTTP 基础设施库，用于构建跨服务行为一致的 API 客户端。

它基于 `reqwest` 构建，封装了 API 客户端最常见的基础能力：请求构建、超时、重试、取消、流式响应、SSE、日志和统一错误处理。

重试计数表示本库的外层尝试次数，不等于线上发送次数；`reqwest` 的安全协议恢复仍保持启用。

文档：[中文用户指南](doc/user_guide.zh_CN.md) | [English User Guide](doc/user_guide.en.md) | [设计文档](doc/design.zh_CN.md) | [Design](doc/design.md) | [API 文档](https://docs.rs/qubit-http)

已准入的重试流程会依据 request interceptor 修改后的最终方法和覆盖项决定是否续试。IPv4-only 客户端会将指向 IPv6 字面量的重定向作为 `InvalidUrl` 拒绝。显式代理的 host 只能填写域名或 IP 字面量，端口和凭据应放在各自配置项中。multipart `Content-Type` 只能有一个 `boundary` 参数，且须与 body 分隔符一致。普通 HTTP 自动重试失败时，请使用 `retry_diagnostics()` 和保留的 `RetryError<HttpError>` source；旧 `retry_metadata()` getter 已弃用且始终返回空值。

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
qubit-http = "0.16"
qubit-redact = "0.10"
http = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

用户指南中的部分示例会用到 `serde`、`serde_json`、`futures-util`、`qubit-config` 等可选辅助依赖。

## 快速开始

这个示例使用 `httpbin.org`，不需要额外启动本地测试服务。

```rust
use http::Method;
use qubit_http::{HttpClientBuilder, HttpClientOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = HttpClientOptions::new();
    options.set_base_url("https://httpbin.org")?;
    options.add_header("x-client-id", "demo")?;

    let client = HttpClientBuilder::new().create(options)?;

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

标准策略会保留 URL path 便于诊断；如果 path 可能含有敏感标识符，请使用
`RedactionPolicy::strict()` 或 `UrlPathPolicy::Redact`。`HttpError` 的 `Debug` 与
`Display` 使用同一套脱敏策略。策略 floor、快照、预算、配置及示例见[日志脱敏指南](doc/user_guide.zh_CN.md#日志脱敏)。
请求会保留构造时捕获的脱敏策略，即使由另一个客户端执行也是如此；需要新策略时请重新构建请求。详见[日志脱敏指南](doc/user_guide.zh_CN.md#日志脱敏)。

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
| `HttpClientBuilder` | 通过默认配置、显式配置或配置中心创建客户端。 |
| `HttpClientOptions` | 保存客户端级默认配置，包括 base URL、请求头、超时、重试、日志、代理、重定向、连接池、JSON value 预算和 SSE 解码。 |
| `HttpClient` | 执行请求，并应用请求头、注入器、拦截器、重试、日志和 SSE 重连辅助能力。 |
| `HttpRequestBuilder` | 构建方法、路径、查询参数、请求头、请求体和请求级覆盖项。 |
| `HttpResponse` | 提供响应元数据，以及 bytes、text、JSON、流式响应和 SSE 的惰性读取方法。 |
| `HttpResponseInterceptorContext` | 让响应拦截器检查 status/method，并修改 headers/最终 URL，同时不破坏成功状态不变量。 |

修改客户端选项且需要保留注入器和拦截器时，使用 `HttpClient::rebuild_with_options`；`to_builder()` 只复制选项。

## 项目范围

- `qubit-http` 基于 `reqwest` 构建，重点是提供稳定、统一的 HTTP 基础设施层，而不是暴露 `reqwest` 的全部 API。
- 响应体始终惰性读取。TRACE 响应体日志只记录已经缓存的 body，不会为了
  日志消费尚未读取的后端流。
- 内置请求重试只覆盖返回 `HttpResponse` 之前的失败。返回后的流式响应体错误会交给调用方处理。
- SSE 重连使用独立 API：`HttpClient::execute_sse_with_reconnect(...)`。
- 已缓存的响应体可重复读取：重复调用 `bytes` / `text` / `json`，以及基于缓存创建 `stream()`，不会移交或清空缓存。
- 标准脱敏策略会保留 URL path；若其中可能包含敏感标识符，可选择严格策略或 path 脱敏。详见[日志脱敏指南](doc/user_guide.zh_CN.md#日志脱敏)。
- SSE 重连有独立的游标、retry 提示和 204 结束语义，详见 [SSE 自动重连](doc/user_guide.zh_CN.md#sse-自动重连)；这不代表完整实现 EventSource。
- 错误预览与保留 raw body 使用独立限额；预览截断不会将完整 raw body 标记为不完整。`read_timeout` 限制完整的非成功错误体采集阶段；超时时，`Status` 错误仍保留 status 和标记为不完整的部分 raw body。
- SSE data 事件只在空行处派发；EOF 会丢弃未完成帧，服务端必须用空行结束最后一个事件。流起始处的一个 BOM 会被忽略。`AllowEof` 允许完整事件流没有 done marker，但不会放宽 framing；`RequireDoneMarker` 只接受完整事件中的 marker。帧和行限额见 [SSE 解码](doc/user_guide.zh_CN.md#sse-解码)。
- 自动 SSE 解码诊断不包含原始 event 或 cursor；Lenient 日志只记录是否存在及字节数。
- 请求与重定向默认使用 `SameOrigin`；确需跨源时显式设为 `AnyOrigin`。配置键 `origin_policy` 接受 `same_origin` 或 `any_origin`。
- JSON SSE 调用方可以通过 `SseCompletionPolicy::RequireDoneMarker` 要求显式完成标记，
  详见 [SSE 指南](doc/user_guide.zh_CN.md#sse-json-chunk)。

重试预算、超时阶段、重放约束、诊断和保留的 `RetryError<HttpError>` source 契约见[自动重试](doc/user_guide.zh_CN.md#自动重试)及[错误模型](doc/user_guide.zh_CN.md#错误模型)。响应头期限在同步请求拦截器运行后开始；`request_timeout` 限制 reqwest 单次网络请求和响应体读取。

## 测试

```bash
# 使用默认 feature 集运行测试
cargo test

# 使用项目声明的全部 feature 运行测试
cargo test --all-features

# 运行项目 CI 检查
./.infra/bin/ci-check.sh

# 检查代码覆盖率
./.infra/bin/coverage.sh
```

## 许可证

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

本项目基于 Apache License 2.0 授权。完整许可证文本请参阅
[LICENSE](LICENSE)。

## 贡献

欢迎贡献。请遵循 Rust API 指南，及时更新公共 API 文档与测试，并在提交
Pull Request 前运行 `./.infra/bin/align-ci.sh`格式化代码，运行`./.infra/bin/ci-check.sh`对齐CI要求。

## 作者

**Haixing Hu** - *Qubit Co. Ltd.*

仓库地址：[https://github.com/qubit-ltd/rs-http](https://github.com/qubit-ltd/rs-http)
