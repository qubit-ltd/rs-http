# `rust-http` 实现方案（内置 SSE 能力）

## 版本信息

- 文档版本：`v1.3`
- 创建日期：`2026-04-08`
- 最近同步：`2026-04-09`（与 `src/` 目录及公开类型落盘一致）
- 目标目录：`rust-common/rust-http/doc`

## 1. 背景与目标

`qubit-http`（crate：`rust-http`）是 **与具体业务解耦的通用 HTTP 基础设施**：为 Qubit 单仓及外部 Rust 组件提供可复用的客户端语义，不绑定某一 SDK 或单一业务线。

本模块目标不是“再造 HTTP 客户端”，而是沉淀统一网络语义：

1. 统一 HTTP 选项与默认值（超时、代理、日志、敏感头、IPv4-only）。
2. 统一请求构建与 header 注入机制（含认证头）。
3. 统一错误类型和可重试语义。
4. 提供流式响应与 SSE 解码能力，供任意需要消费 SSE 的调用方复用。

## 2. 基本需求分析（能力边界 + 默认值约定 + 当前决策）

### 2.1 能力边界（硬性要求）

与 `doc/http_prd.zh_CN.md` 一致，本模块须满足：

- 必须提供：`HttpClientOptions`、`HttpClientFactory`
- 必须支持：默认 header 注入、代理、connect/read/write timeout、日志开关、敏感头脱敏、统一错误、流式入口
- 技术栈约束：`reqwest` + `http` + `url` + `bytes`
- 边界约束：不重复实现协议栈，不做 `reqwest` 全量转发层；本 crate **不反向依赖**具体业务 crate

### 2.2 默认值与行为约定

以下为 **本设计采用的默认配置与行为**（在 PRD/用户文档中明示，便于各调用方预期一致）：

1. 默认值：
   - `connection_timeout = 10s`
   - `read_timeout = 120s`
   - `write_timeout = 120s`
   - `use_proxy = false`
   - `proxy_type = http`
   - `use_http_logging = true`
   - 请求/响应头体日志开关默认均 `true`
   - `ipv4_only = false`
2. 敏感头默认列表：如 `Authorization`、`Api-Key`、`Cookie`、`Set-Cookie` 等。
3. 脱敏策略：`<=4` 字符全掩码，否则保留前2后2。
4. 代理支持：类型 + host/port + username/password。
5. 流式处理中存在固定 SSE 语义：`data:`、空行、`[DONE]`、坏 chunk 容错。

### 2.3 当前架构决策

本轮按项目决策将 SSE 能力内聚到 `rust-http::sse` 子模块，不再单独拆 `qubit-sse`。  
这样可减少短期模块拆分成本，并保持调用方接入路径最短。

## 3. 非目标（边界冻结）

1. 不封装 `reqwest` 全部 API，不做“二次转发 SDK”。
2. 不在 `rust-http` 中实现非 HTTP 协议流解析（如 WebSocket/gRPC）。
3. 不在 `rust-http` 内固化完整重试策略（只提供与 `qubit-retry` 的衔接点）。

## 4. 总体架构

```text
调用方（服务 / 库）
      |
      v
  qubit-http
  |-- options (统一配置)
  |-- factory (客户端构造)
  |-- request/response (最小请求抽象)
  |-- logging (日志与脱敏)
  |-- stream (字节流入口)
  |-- sse (SSE 协议解码)
  |-- error (统一错误模型)
      |
      v
   reqwest/http/url/bytes
```

## 5. 核心 API 设计

### 5.1 配置对象

```rust
pub struct HttpClientOptions {
    pub base_url: Option<url::Url>,
    pub default_headers: http::HeaderMap,
    pub timeouts: TimeoutOptions,
    pub proxy: ProxyOptions,
    pub logging: HttpLoggingOptions,
    pub sensitive_headers: SensitiveHeaders,
    pub ipv4_only: bool,
}

pub struct TimeoutOptions {
    pub connect_timeout: std::time::Duration,
    pub read_timeout: std::time::Duration,
    pub write_timeout: std::time::Duration,
    pub request_timeout: Option<std::time::Duration>,
}

pub struct ProxyOptions {
    pub enabled: bool,
    pub proxy_type: ProxyType,      // Http | Https | Socks5
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
}
```

默认值与 §2.2 一致：`10/120/120` 秒、`use_proxy=false`、`use_http_logging=true`、`ipv4_only=false`。

### 5.2 客户端工厂

```rust
pub struct HttpClientFactory;
```

- `HttpClientFactory` 当前明确是基于 `reqwest` 的具体工厂类型。
- 工厂负责把统一 options 映射到底层 `reqwest::ClientBuilder`。

### 5.3 请求与响应

```rust
pub struct HttpClient { /* 内含 reqwest::Client + Arc<HttpClientOptions> */ }

impl HttpClient {
    pub fn request(&self, method: http::Method, path: impl AsRef<str>) -> HttpRequestBuilder;
    pub async fn execute(&self, req: HttpRequest) -> Result<HttpResponse, HttpError>;
    pub async fn execute_stream(&self, req: HttpRequest) -> Result<HttpStreamResponse, HttpError>;
}
```

设计原则：

1. 只保留高频核心接口（`request/execute/execute_stream`）。
2. `HttpRequestBuilder` 支持 path/query/header/json/body，不暴露全量底层 API。
3. SSE 解码在 `sse` 子模块中完成，复用 `execute_stream` 的字节流。

### 5.4 Header 注入机制

```rust
pub trait HeaderInjector: Send + Sync {
    fn inject(&self, headers: &mut http::HeaderMap) -> Result<(), HttpError>;
}
```

注入顺序：

1. `options.default_headers`
2. `HeaderInjector`（认证头、租户/组织头等，由调用方实现具体策略）
3. 请求级 headers（最后覆盖）

### 5.5 SSE 子模块（`rust-http::sse`）

```rust
pub struct SseEvent { /* sse/sse_event.rs */ }

// `HttpResult<T> = Result<T, HttpError>` 见 `error/http_result.rs`
pub type SseEventStream =
    Pin<Box<dyn Stream<Item = Result<SseEvent, HttpError>> + Send>>;

pub enum DoneMarkerPolicy { /* sse/done_marker_policy.rs */
    Disabled,
    DefaultDone,          // [DONE]
    Custom(String),
}

pub enum SseChunk<T> { /* sse/sse_chunk.rs */
    Data(T),
    Done,
}

pub enum SseJsonMode { /* sse/sse_json_mode.rs */
    Lenient,
    Strict,
}

pub type SseChunkStream<T> =
    Pin<Box<dyn Stream<Item = Result<SseChunk<T>, HttpError>> + Send>>;

pub fn decode_events(stream: HttpStreamResponse) -> SseEventStream;

pub fn decode_json_chunks<T: serde::de::DeserializeOwned + Send + 'static>(
    stream: HttpStreamResponse,
    done_policy: DoneMarkerPolicy,
) -> SseChunkStream<T>;

pub fn decode_json_chunks_with_mode<T: serde::de::DeserializeOwned + Send + 'static>(
    stream: HttpStreamResponse,
    done_policy: DoneMarkerPolicy,
    mode: SseJsonMode,
) -> SseChunkStream<T>;
```

行为约束：

1. 支持 `data:` 前缀解析。
2. 支持空行 frame 分隔。
3. 支持 `[DONE]` 结束标记。
4. 支持坏 JSON chunk 跳过（可配置严格/宽松模式，默认宽松）。

## 6. 日志与敏感信息策略

### 6.1 日志开关

`HttpLoggingOptions` 字段约定如下（与 §2.2 默认策略一致）：

1. `enabled`
2. `log_request_header`
3. `log_request_body`
4. `log_response_header`
5. `log_response_body`

### 6.2 敏感头脱敏

1. 内置常见敏感头默认集合（如 `Authorization`、`Api-Key`、`Cookie` 等，可配置扩展）。
2. header 名称大小写不敏感匹配。
3. 掩码策略：长度 `<=4` 返回 `****`，否则 `前2 + **** + 后2`。

### 6.3 日志安全约束

1. body 仅在 `trace` 级别输出。
2. 二进制/未知编码 body 不打印原文，只打印大小。
3. body 打印长度设置上限（建议默认 `16KB`），防止日志膨胀。

## 7. 错误模型与重试语义

### 7.1 统一错误类型

```rust
pub enum HttpErrorKind { /* error/http_error_kind.rs */
    InvalidUrl,
    BuildClient,
    ProxyConfig,
    ConnectTimeout,
    ReadTimeout,
    WriteTimeout,
    Transport,
    Status,        // 非 2xx
    Decode,
    SseProtocol,
    SseDecode,
    Cancelled,
    Other,
}

// `source` 字段类型为 `qubit_common::BoxError`（与 `Box<dyn std::error::Error + Send + Sync>` 等价）

pub struct HttpError { /* error/http_error.rs */
    pub kind: HttpErrorKind,
    pub method: Option<http::Method>,
    pub url: Option<url::Url>,
    pub status: Option<http::StatusCode>,
    pub message: String,
    pub source: Option<BoxError>,
}
```

配置转换错误（`config` feature）与运行时 `HttpError` 分层：

```rust
pub enum HttpConfigErrorKind { /* http_config_error_kind.rs */ }

pub struct HttpConfigError { /* http_config_error.rs */ }
```

### 7.2 Retry 衔接点（面向 `qubit-retry`）

```rust
pub enum RetryHint { Retryable, NonRetryable }
impl HttpError { pub fn retry_hint(&self) -> RetryHint { ... } }
```

建议默认判定：

1. `ConnectTimeout/ReadTimeout/WriteTimeout/Transport` => `Retryable`
2. `Status` 且 `429/5xx` => `Retryable`
3. `SseProtocol/SseDecode/InvalidUrl/ProxyConfig` => `NonRetryable`
4. 其余 => `NonRetryable`

## 8. 与 `reqwest` 的映射策略

### 8.1 直接映射

1. `connect_timeout` -> `ClientBuilder::connect_timeout`
2. `request_timeout` -> `ClientBuilder::timeout`
3. `proxy` -> `ClientBuilder::proxy(...)`
4. `default_headers` -> `ClientBuilder::default_headers`

### 8.2 兼容实现（read/write timeout）

`reqwest` 对“读写超时分离”没有一对一原生模型，方案如下：

1. `read_timeout`：读取响应 body（含 stream chunk）时用 `tokio::time::timeout` 包装。
2. `write_timeout`：请求发送阶段（直到首包/响应头）应用 `tokio::time::timeout`。
3. `request_timeout`：作为总兜底超时（可选）。

### 8.3 SSE 映射策略

1. 上游输入：`execute_stream` 输出的 `bytes::Bytes` 流。
2. 行切分：按 `\n` 处理，兼容 `\r\n`。
3. 事件拼装：空行视为 frame 结束，支持多行 `data:` 聚合。
4. DONE 识别：支持 `[DONE]` 或自定义 done marker。
5. 容错：宽松模式下坏 JSON chunk 跳过，不中断整条流。

### 8.4 IPv4-only

分阶段实现：

1. M1：字段与校验先落地（API 稳定）。
2. M2：通过可插拔 DNS resolver 实现“仅解析/使用 IPv4”。

## 9. 目录与模块落盘（与当前 `src/` 一致）

约定：**公开类型**尽量 **一类型一文件**（蛇形文件名与类型名对应）；`mod.rs` 仅做聚合与再导出；实现细节（如仅含函数的解码步骤）可保留在独立模块中。

```text
rust-common/rust-http/
  ├─ src/
  │   ├─ lib.rs
  │   ├─ http_client.rs
  │   ├─ constants.rs
  │   ├─ retry_hint.rs
  │   ├─ http_config_error_kind.rs
  │   ├─ http_config_error.rs
  │   ├─ config_impl.rs              # feature `config`：from_config / validate 实现
  │   ├─ error/
  │   │   ├─ mod.rs
  │   │   ├─ http_error_kind.rs
  │   │   ├─ http_error.rs
  │   │   └─ http_result.rs
  │   ├─ factory/
  │   │   ├─ mod.rs
  │   │   ├─ http_client_factory.rs
  │   │   └─ reqwest_http_client_factory.rs
  │   ├─ options/
  │   │   ├─ mod.rs
  │   │   ├─ http_client_options.rs
  │   │   ├─ timeout_options.rs
  │   │   ├─ proxy_options.rs
  │   │   ├─ proxy_type.rs
  │   │   ├─ logging_options.rs
  │   │   └─ sensitive_headers.rs
  │   ├─ request/
  │   │   ├─ mod.rs
  │   │   ├─ http_request.rs
  │   │   ├─ http_request_builder.rs
  │   │   ├─ http_request_body.rs
  │   │   └─ header_injector.rs
  │   ├─ response/
  │   │   ├─ mod.rs
  │   │   └─ http_response.rs
  │   ├─ stream/
  │   │   ├─ mod.rs
  │   │   ├─ http_byte_stream.rs
  │   │   └─ http_stream_response.rs
  │   ├─ logging/
  │   │   ├─ mod.rs
  │   │   ├─ policy.rs
  │   │   └─ masker.rs
  │   └─ sse/
  │       ├─ mod.rs                  # decode_events
  │       ├─ line_decoder.rs
  │       ├─ frame_decoder.rs
  │       ├─ done_marker_policy.rs
  │       ├─ sse_event.rs
  │       ├─ sse_event_stream.rs
  │       ├─ sse_chunk.rs
  │       ├─ sse_json_mode.rs
  │       ├─ sse_chunk_stream.rs
  │       └─ json_decoder.rs         # decode_json_chunks / decode_json_chunks_with_mode
  └─ doc/
      ├─ http_design.zh_CN.md
      ├─ http_v0.2.0_design.zh_CN.md
      └─ http_prd.zh_CN.md
```

## 10. 分阶段落地计划（可直接执行）

### 阶段 A：骨架与最小可用

1. 建立 crate 与 `HttpClientOptions/HttpClientFactory/HttpClient` 基础类型。
2. 打通 `request + execute`（JSON 请求可用）。
3. 建立统一 `HttpError`。

验收：可完成普通 JSON API 调用，并返回结构化错误。

### 阶段 B：日志与脱敏

1. 完成日志策略与敏感头掩码。
2. 完成 header 注入链。
3. 补齐默认敏感头集合。

验收：日志开关生效，敏感头不泄露明文。

### 阶段 C：流式、SSE 与超时细化

1. 提供 `execute_stream`。
2. 完成 `SseEvent` 行/帧解码与 `[DONE]` 识别。
3. 完成 `decode_json_chunks<T>`（默认宽松模式）。
4. 完成 read/write timeout 包装逻辑与 `RetryHint` 判定。

验收：可稳定消费 SSE 并输出强类型 chunk，超时与错误分类准确。

### 阶段 D：增强与对齐

1. 实现 IPv4-only（resolver 插件化）。
2. 增加与 `qubit-retry` 的样例适配。
3. 补全集成测试矩阵。

验收：默认值、超时、日志与 SSE 行为与本文 §2.2 / PRD 可追溯一致，具备跨业务复用能力。

## 11. 测试策略

1. 单元测试：
   - 默认值、配置解析、proxy 校验、敏感头掩码、错误分类。
   - SSE 行切分、frame 拼装、done marker、JSON chunk 容错。
2. 集成测试（建议 `wiremock`/本地测试服务）：
   - 2xx / 4xx / 5xx、超时、代理、流式响应、SSE 解析、日志分支。
3. 回归测试：
   - `RetryHint` 与状态码映射；
   - `execute_stream` 在坏块/断流下的错误语义稳定性；
   - `decode_json_chunks` 在异常 chunk 下不中断整流。

## 12. 能力清单对照（PRD + 本轮扩展）

1. 定义客户端选项结构：`HttpClientOptions` + 子配置对象。
2. 定义基于 `reqwest` 的客户端工厂：`HttpClientFactory`。
3. 定义 header 注入和脱敏策略：`HeaderInjector` + `SensitiveHeaders`。
4. 定义统一 timeout 和 proxy 配置：`TimeoutOptions` + `ProxyOptions`。
5. 定义统一 HTTP error 类型：`HttpError/HttpErrorKind`。
6. 预留与 `qubit-retry` 衔接点：`RetryHint` + `HttpError::retry_hint()`。
7. 在 `rust-http::sse` 内提供 SSE 解码能力：`SseEvent`、`DoneMarkerPolicy`、`SseChunk`/`SseJsonMode`、行/帧解码与 `decode_json_chunks` 系列函数。

---

该方案满足 PRD 与 §2.1 能力边界，并按当前决策将 SSE 内聚到 `rust-http`，避免各调用方重复实现流式解析与日志脱敏等横切逻辑。
