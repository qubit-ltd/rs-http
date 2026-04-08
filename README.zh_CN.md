# Qubit HTTP（`rust-http`）

[![CircleCI](https://circleci.com/gh/qubit-ltd/rust-http.svg?style=shield)](https://circleci.com/gh/qubit-ltd/rust-http)
[![Coverage Status](https://coveralls.io/repos/github/qubit-ltd/rust-http/badge.svg?branch=main)](https://coveralls.io/github/qubit-ltd/rust-http?branch=main)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
[![Rust](https://img.shields.io/badge/rust-1.70+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

通用的 Rust HTTP 基础设施模块，内置 SSE 解码能力，并提供统一配置与一致错误语义。

## 当前状态

本仓库目前处于**设计先行阶段**。

- 产品需求文档：[`doc/http_prd.zh_CN.md`](doc/http_prd.zh_CN.md)
- 技术设计文档：[`doc/http_design.zh_CN.md`](doc/http_design.zh_CN.md)

下一步将进入实现阶段（见下方 M1/M2 里程碑）。

## 目标

1. 为应用与服务模块提供统一 HTTP 客户端抽象。
2. 统一超时、代理、日志、敏感头等行为语义。
3. 将 SSE 能力内聚到 `rust-http::sse`（不再拆分独立模块）。
4. 提供统一错误模型与重试提示，便于接入 `qubit-retry`。

## 范围

### 包含

- `HttpClientOptions`、`HttpClientFactory` 及默认 `reqwest` 实现
- 普通请求执行（`execute`）与流式执行（`execute_stream`）
- Header 注入链路
- 代理能力（含代理认证）
- connect/read/write/request timeout 语义
- 日志开关与敏感头脱敏
- SSE 解码（`data:`、分帧、`[DONE]`、JSON chunk 解码）
- 统一 `HttpError` 与 `RetryHint`

### 不包含

- 对 `reqwest` 全量 API 的转发封装
- 非 HTTP 协议流解析（WebSocket/gRPC 等）
- 在本模块内实现完整重试策略引擎

## 计划模块结构

```text
rust-http/
  ├─ src/
  │   ├─ options.rs
  │   ├─ factory.rs
  │   ├─ client.rs
  │   ├─ request.rs
  │   ├─ response.rs
  │   ├─ stream.rs
  │   ├─ error.rs
  │   ├─ retry_hint.rs
  │   ├─ logging/
  │   └─ sse/
  └─ doc/
```

## 里程碑

### M1（P0）

- 核心 HTTP 抽象与默认工厂
- 流式入口与 SSE 解码
- 统一错误映射与重试提示
- 日志脱敏、超时和代理能力

### M2（P1）

- IPv4-only resolver 策略
- 可观测性增强与回归测试扩展

## 说明

- 本项目刻意保持“小而稳”的抽象边界，避免过度封装。
- 设计目标是跨业务场景复用，而不是替代底层 HTTP crate。
