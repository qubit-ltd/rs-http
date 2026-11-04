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

运行共享的本地示例：`cargo run --example local_quick_start`。示例会在操作系统分配的端口启动回环服务，把该地址设为 `base_url`，发送带客户端标识和查询参数的 GET 请求，并检查响应：

```rust
let mut options = HttpClientOptions::new();
options.set_base_url(&format!("http://{address}"))?;
options.add_header("x-client-id", "demo")?;
let client = HttpClient::builder().options(options).build()?;
let request = client
    .request(Method::GET, "/anything")
    .query_param("from", "local-quick-start")
    .build();
let mut response = client.execute(request).await?;
println!("status = {}", response.status());
println!("text = {}", response.text().await?);
```

包含本地服务的完整可运行源码见 [`examples/local_quick_start.rs`](examples/local_quick_start.rs)。运行结果：

```text
status = 200 OK
text = ready
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
| 从 `qubit-http` 0.15 升级到 0.16 | [升级指南](doc/user_guide.zh_CN.md#从-015-升级到-016) |

## 核心 API 概览

默认配置使用 `HttpClient::builder().build()`，显式选项使用
`HttpClient::builder().options(options).build()`，配置读取使用
`HttpClient::from_config(config)`。旧的 `HttpClientBuilder::create*` 方法仍保留，
但已弃用。特别是，`.options(saved).create_default()` 会忽略 `saved`；若要应用这些
选项，请使用 `.options(saved).build()`。

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

- `qubit-http` 基于 `reqwest` 提供稳定、统一的 HTTP 基础设施接口。
- `execute` 只对 2xx 状态返回响应。响应体采用惰性读取；TRACE body 日志不会消费尚未读取的后端流。
- 内置请求重试只覆盖返回 `HttpResponse` 之前的失败；返回后的流读取错误由调用方处理。SSE 重连使用独立 API 和生命周期。详见[自动重试](doc/user_guide.zh_CN.md#自动重试)及 [SSE 自动重连](doc/user_guide.zh_CN.md#sse-自动重连)。
- 默认使用 `SameOrigin`。初始 URL 按请求快照校验，重定向按执行客户端的后端策略校验。详见[跨客户端来源策略](doc/user_guide.zh_CN.md#跨客户端来源策略)。
- 标准脱敏策略保留 URL path；若其中可能包含敏感信息，请使用严格或 path 脱敏。详见[日志脱敏](doc/user_guide.zh_CN.md#日志脱敏)。
- SSE 重连有独立的游标、retry 提示和 204 结束语义，不代表完整实现 EventSource。详见 [SSE 自动重连](doc/user_guide.zh_CN.md#sse-自动重连)。
- 错误预览、保留 raw body 的限额和错误体完整读取期限相互独立。详见[错误模型](doc/user_guide.zh_CN.md#错误模型)。

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
