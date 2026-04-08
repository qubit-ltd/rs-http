# Qubit HTTP (`rust-http`)

[![CircleCI](https://circleci.com/gh/qubit-ltd/rust-http.svg?style=shield)](https://circleci.com/gh/qubit-ltd/rust-http)
[![Coverage Status](https://coveralls.io/repos/github/qubit-ltd/rust-http/badge.svg?branch=main)](https://coveralls.io/github/qubit-ltd/rust-http?branch=main)
[![Crates.io](https://img.shields.io/crates/v/qubit-http.svg?color=blue)](https://crates.io/crates/qubit-http)
[![Rust](https://img.shields.io/badge/rust-1.70+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

A general-purpose Rust HTTP infrastructure module with built-in SSE decoding, unified configuration, and consistent error semantics.

## Status

This repository is currently in the **design-first phase**.

- Product requirements: [`doc/http_prd.zh_CN.md`](doc/http_prd.zh_CN.md)
- Technical design: [`doc/http_design.zh_CN.md`](doc/http_design.zh_CN.md)

Implementation is planned next (M1/M2 milestones below).

## Goals

1. Provide a unified HTTP client abstraction for application and service modules.
2. Standardize timeout/proxy/logging/sensitive-header behavior.
3. Provide built-in SSE decoding in `rust-http::sse` (instead of a separate module).
4. Expose a consistent error model and retry hints for `qubit-retry` integration.

## Scope

### In scope

- `HttpClientOptions`, `HttpClientFactory`, and a default `reqwest`-based implementation
- Request execution (`execute`) and streaming execution (`execute_stream`)
- Header injection pipeline
- Proxy + auth proxy support
- Connect/read/write/request timeout behavior
- Logging toggles + sensitive header masking
- SSE decoding (`data:`, frame boundary, `[DONE]`, JSON chunk decoding)
- Unified `HttpError` and `RetryHint`

### Out of scope

- Wrapping the full `reqwest` API surface
- Parsing non-HTTP streaming protocols (WebSocket/gRPC/etc.)
- Embedding a full retry policy engine in this module

## Planned Module Layout

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

## Roadmap

### M1 (P0)

- Core HTTP abstractions and default client factory
- Streaming entry + SSE decoding
- Unified error mapping and retry hints
- Logging/masking and timeout/proxy behavior

### M2 (P1)

- IPv4-only resolver strategy
- Enhanced observability and expanded regression coverage

## Notes

- This project intentionally keeps a small abstraction boundary.
- The module is designed for cross-domain reuse in Rust service stacks.
