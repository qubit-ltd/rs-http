# `qubit-http` 内嵌 `qubit-retry` 设计文档

## 文档信息

- 文档名称：`qubit-http 内嵌 qubit-retry 设计文档`
- 文档版本：`v1.0`
- 创建日期：`2026-04-12`
- 适用范围：`qubit-http`、`qubit-retry`
- 目标：在 `HttpClient` 中内建可配置、可观测、语义安全的 HTTP retry 机制

## 1. 背景

`qubit-http` 当前已经提供统一错误模型和 `RetryHint`：

1. `ConnectTimeout`、`ReadTimeout`、`WriteTimeout`、`Transport` 默认为可重试。
2. `Status` 且状态码为 `429` 或 `5xx` 默认为可重试。
3. `InvalidUrl`、`ProxyConfig`、`SseProtocol`、`SseDecode` 等默认为不可重试。

但当前 retry 仅停留在“提示”层，调用方需要自己把 `HttpError::retry_hint()` 接到 `qubit-retry`。这会导致不同业务方重复实现 retry glue code，并且容易在以下方面分叉：

1. HTTP 方法是否允许重试。
2. 非 2xx 状态码是否重试。
3. 最大尝试次数和 backoff 参数。
4. 最终错误应该保留原始 `HttpError` 还是转为 retry 框架错误。
5. 流式响应应该在哪个阶段允许 retry。

因此需要把 retry 机制内嵌到 `HttpClient`，让默认行为统一，同时仍然保留显式配置入口。

## 2. 当前实现分析

### 2.1 `qubit-http`

当前 `HttpClient::execute()` 是单次请求路径：

1. 解析请求 URL。
2. 合并默认 headers、header injector 和 request headers。
3. 记录请求日志。
4. 构建 `reqwest::RequestBuilder`。
5. 调用 `send_with_write_timeout()`。
6. 非成功状态码直接返回 `HttpError::status(...)`。
7. 调用 `read_body_with_timeout()` 读取完整 body。
8. 记录响应日志并返回 `HttpResponse`。

`HttpClient::execute_stream()` 也是单次请求路径，只是在成功拿到初始响应后返回 `HttpStreamResponse`，后续 body chunk 读取错误在 stream item 中返回。

`HttpRequest` 和 `HttpRequestBody` 都是 `Clone`，并且 body 类型是 `Empty`、`Bytes`、`Text`、`Json`，没有 one-shot reader。这意味着从数据结构上看，请求可以重放。

### 2.2 `qubit-retry`

当前 `RetryExecutor<T>` 对成功值 `T` 的约束是：

```rust
T: Clone + PartialEq + Eq + std::hash::Hash + Send + Sync + 'static
```

这个约束适合“基于返回值判断是否失败”的 retry 场景，但不适合直接包裹 HTTP：

1. `HttpResponse` 当前只有 `Debug + Clone`，不适合强行实现 `Eq + Hash`。
2. `HttpStreamResponse` 包含 stream，本质上不可 clone，也不应该做 equality/hash。
3. HTTP retry 主要基于错误值 `HttpError` 的内容判断，而不是基于成功值判断。

此外，`RetryBuilder` 当前的错误类型过滤是粗粒度的。配置了某个错误类型后，内部并未真正基于实际错误值做严格判断，也没有提供 `Fn(&dyn Error) -> bool` 形式的错误 predicate。HTTP 需要的是：

```rust
|error: &HttpError| matches!(error.retry_hint(), RetryHint::Retryable)
```

因此，若不修改 `qubit-retry`，`qubit-http` 会被迫自己维护 retry loop，或者在 `RetryExecutor<()>` 旁边用外部变量保存最后一次 `HttpError`。这两种方式都不够清晰。

## 3. 设计目标

1. `HttpClient` 内建 retry，不要求业务层重复 glue code。
2. 默认行为保持向后兼容：未显式启用 retry 时仍然只请求一次。
3. 默认只重试 `HttpError::retry_hint() == RetryHint::Retryable` 的错误。
4. 默认只自动重试幂等 HTTP 方法，避免误重放非幂等请求。
5. 最终失败时优先返回最后一次原始 `HttpError`，保留 method、url、status、source 等诊断信息。
6. retry delay、max attempts、max duration、jitter 等能力复用 `qubit-retry`。
7. `execute_stream()` 只在初始响应返回前重试，不对已交付给调用方的 stream 中途自动重试。
8. 实现边界清晰：`qubit-http` 只定义 HTTP retry 语义，通用 retry 调度仍由 `qubit-retry` 提供。

## 4. 非目标

1. 不封装 `reqwest` 全量 retry 能力。
2. 不自动重试已经部分消费的 streaming body。
3. 不默认重试 `POST`、`PATCH` 等非幂等请求。
4. 不在 `qubit-http` 中重写一套通用 retry 框架。
5. 不要求 `HttpResponse` 或 `HttpStreamResponse` 实现 `Eq` / `Hash`。

## 5. 总体方案

推荐方案分两层实现：

1. 先增强 `qubit-retry`：新增“基于错误策略的 async retry runner”，放宽对成功值 `T` 的 `Eq + Hash` 约束，并保留最后一次原始错误。
2. 再在 `qubit-http` 中新增 `HttpRetryOptions`，由 `HttpClient::execute()` 和 `HttpClient::execute_stream()` 调用 retry runner。

整体调用路径：

```text
HttpClient::execute(request)
  |
  |-- retry 未启用或方法不允许 retry
  |     |
  |     `-- execute_once(request)
  |
  `-- retry 启用且方法允许 retry
        |
        `-- RetryExecutor::run_async_error_policy(...)
              |
              |-- attempt #1: execute_once(request.clone())
              |-- Err(HttpError) + retry_hint=Retryable => delay + next attempt
              |-- Err(HttpError) + retry_hint=NonRetryable => return original HttpError
              |-- Ok(HttpResponse) => return response
              `-- attempts exhausted => return last original HttpError
```

## 6. `qubit-retry` 设计调整

### 6.1 新增错误策略 runner

新增 API 方向：

```rust
pub enum RetryRunError<E> {
    Aborted {
        error: E,
    },
    MaxAttemptsExceeded {
        attempts: u32,
        max_attempts: u32,
        last_error: E,
    },
    MaxDurationExceeded {
        duration: Duration,
        max_duration: Duration,
        last_error: Option<E>,
    },
    OperationTimeout {
        duration: Duration,
        timeout: Duration,
    },
}

impl<C> RetryExecutor<(), C>
where
    C: RetryConfig,
{
    pub async fn run_async_error_policy<T, E, F, Fut, P>(
        &self,
        operation: F,
        should_retry: P,
    ) -> Result<T, RetryRunError<E>>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: Error + Send + Sync + 'static,
        P: Fn(&E) -> bool;
}
```

说明：

1. `T` 不再要求 `Clone + Eq + Hash`，因为该 runner 不做“结果值失败判定”。
2. `E` 是原始错误类型，例如 `HttpError`。
3. `should_retry(&E)` 决定当前错误是否进入下一次尝试。
4. 达到最大尝试次数时返回最后一次原始错误，不丢失业务错误上下文。
5. `max_duration` 超时时，如果已经有失败错误，则通过 `last_error` 暴露；如果在第一次尝试前就超时，则为 `None`。
6. 单次 operation timeout 可继续复用 `tokio::time::timeout`，但对 `qubit-http` 建议先不重复设置 operation timeout，避免和 HTTP 自己的 connect/read/write/request timeout 语义重叠。

### 6.2 事件语义

该 runner 可以复用现有 retry/success/failure/abort listener，但需要注意类型参数选择。推荐在内部使用 `RetryExecutor<()>` 作为配置载体：

1. retry event 的 result 相关字段为空。
2. last error 可放入 event 的 `last_error`。
3. success event 的 result 使用 `()`，仅表达“最终成功”。
4. failure event 的 last error 使用最后一次原始错误的 display/source 信息。

若现有事件类型无法无损表达 `E`，可以先保持 runner 自身返回 `RetryRunError<E>`，事件只做 best-effort 记录。

### 6.3 与现有 API 的兼容性

不建议修改现有 `run()` / `run_async()` 的签名和行为。新增 runner 是并行能力：

1. 原有基于结果值的 retry 继续使用 `RetryExecutor<T>::run_async()`。
2. HTTP 这类基于错误值的场景使用 `RetryExecutor<()>::run_async_error_policy()`。
3. 避免让 `HttpResponse` 强行实现 `Eq + Hash`。

## 7. `qubit-http` 设计调整

### 7.1 新增 `HttpRetryOptions`

建议新增模块：

```text
src/options/retry_options.rs
```

建议公开类型：

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRetryOptions {
    pub enabled: bool,
    pub max_attempts: u32,
    pub max_duration: Option<Duration>,
    pub delay_strategy: RetryDelayStrategy,
    pub jitter_factor: f64,
    pub method_policy: HttpRetryMethodPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpRetryMethodPolicy {
    IdempotentOnly,
    AllMethods,
    None,
}
```

默认值建议：

```text
enabled       = false
max_attempts  = 3
max_duration  = None
delay_strategy = ExponentialBackoff {
    initial_delay = 200ms,
    max_delay = 5s,
    multiplier = 2.0,
}
jitter_factor = 0.1
method_policy = IdempotentOnly
```

默认 `enabled = false` 是为了最大限度保持向后兼容。如果后续希望“默认启用”，应单独发版本并在 release note 中明确说明。

### 7.2 挂到 `HttpClientOptions`

在 `HttpClientOptions` 中新增字段：

```rust
pub retry: HttpRetryOptions,
```

`Default` 中初始化为 `HttpRetryOptions::default()`。

`from_config()` 支持新配置前缀：

```text
retry.enabled
retry.max_attempts
retry.max_duration
retry.delay_strategy
retry.fixed_delay
retry.random_min_delay
retry.random_max_delay
retry.backoff_initial_delay
retry.backoff_max_delay
retry.backoff_multiplier
retry.jitter_factor
retry.method_policy
```

其中 duration 字段沿用项目已有 `qubit-config` 对 `Duration` 的解析能力，避免引入 `*_millis` 与现有 `timeouts.*` 风格不一致的问题。

### 7.3 HTTP 方法策略

`IdempotentOnly` 下默认允许：

1. `GET`
2. `HEAD`
3. `PUT`
4. `DELETE`
5. `OPTIONS`
6. `TRACE`

默认不允许：

1. `POST`
2. `PATCH`
3. 其他扩展方法

如果业务方确认请求可重放，可将 `method_policy` 设置为 `AllMethods`。

后续增强可以支持 request-level override，例如：

```rust
request.retry_policy(HttpRequestRetryPolicy::ForceEnabled)
```

但第一阶段不建议加入，避免扩大 API 面。

### 7.4 拆分 `execute_once`

将当前 `execute()` 主体下沉为私有方法：

```rust
async fn execute_once(&self, request: HttpRequest) -> HttpResult<HttpResponse>;
```

新的 `execute()` 负责选择是否启用 retry：

```rust
pub async fn execute(&self, request: HttpRequest) -> HttpResult<HttpResponse> {
    if !self.should_retry_request(&request) {
        return self.execute_once(request).await;
    }
    self.execute_with_retry(request).await
}
```

`execute_with_retry()`：

```rust
async fn execute_with_retry(&self, request: HttpRequest) -> HttpResult<HttpResponse> {
    let executor = self.build_retry_executor();
    let result = executor
        .run_async_error_policy(
            || {
                let request = request.clone();
                async move { self.execute_once(request).await }
            },
            |error: &HttpError| matches!(error.retry_hint(), RetryHint::Retryable),
        )
        .await;

    map_retry_run_error(result)
}
```

实际实现时要注意 async closure 对 `self` 和 `request` 的借用关系。若生命周期不够直接表达，可先 clone `HttpClient`：

```rust
let client = self.clone();
let request = request.clone();
async move { client.execute_once(request).await }
```

`reqwest::Client` 可 clone，`HttpClientOptions` 和 `HttpHeaderInjector` 当前也支持 clone，所以该方案可行。

### 7.5 最终错误映射

`execute_with_retry()` 不应把最终错误暴露为 `RetryError`，否则调用方会丢失 HTTP 语义。映射建议：

1. `RetryRunError::Aborted { error }`：返回该 `HttpError`。
2. `RetryRunError::MaxAttemptsExceeded { last_error, .. }`：返回 `last_error`，可在 message 中追加 attempts 信息。
3. `RetryRunError::MaxDurationExceeded { last_error: Some(error), .. }`：返回 `error`，可追加 max duration 信息。
4. `RetryRunError::MaxDurationExceeded { last_error: None, .. }`：返回 `HttpError::other(...)`。
5. `RetryRunError::OperationTimeout { .. }`：返回 `HttpError::read_timeout(...)` 或 `HttpError::write_timeout(...)` 不够准确，建议返回 `HttpError::other(...)`；但第一阶段最好不要为 HTTP retry runner 设置 operation timeout，避免进入该分支。

如果需要追加 retry 上下文，建议新增 `HttpError::with_message_context(...)` 或内部 helper 构造新 message，但不要丢弃 kind/method/url/status/source。

### 7.6 `execute_stream()` 语义

`execute_stream()` 也可拆分：

```rust
async fn execute_stream_once(&self, request: HttpRequest) -> HttpResult<HttpStreamResponse>;
```

retry 只覆盖以下阶段：

1. URL 解析。
2. headers 构建。
3. 请求发送。
4. 初始 HTTP status 判断。
5. 初始 response headers 返回前。

一旦返回 `HttpStreamResponse`，后续 `into_stream()` 产生的 chunk 读取错误不自动 retry。

原因：

1. 调用方可能已经消费部分数据。
2. SSE / chunked stream 的中途重连涉及业务级 offset、event id、幂等语义，不属于通用 HTTP client 能安全处理的范围。
3. 自动拼接 stream 会引入数据重复或丢失风险。

## 8. 日志与可观测性

第一阶段建议复用现有 request/response 日志，不新增独立日志结构：

1. 每次 attempt 都调用现有 `log_request()`。
2. 每次失败由 `qubit-retry` 的 retry listener 记录 attempt、delay、error。
3. 成功响应仍由现有 `log_response()` 或 `log_stream_response_headers()` 记录。

后续可以新增 retry 专用日志选项：

```rust
pub struct HttpLoggingOptions {
    pub log_retry: bool,
    ...
}
```

但第一阶段不建议扩大 logging options，避免和 retry 功能耦合过多。

## 9. 依赖设计

`qubit-http` 需要新增依赖：

```toml
qubit-retry = "0.2.x"
```

如果担心基础依赖面扩大，可以设计 Cargo feature：

```toml
[features]
default = []
retry = ["dep:qubit-retry"]

[dependencies]
qubit-retry = { version = "0.2.x", optional = true }
```

但如果 `HttpClientOptions` 的公开字段直接包含 `RetryDelayStrategy`，那么 feature 会让公开 API 变复杂。推荐第一阶段直接依赖 `qubit-retry`，因为 retry 内嵌后已经成为 `qubit-http` 的核心能力。

## 10. 配置示例

```toml
[http.retry]
enabled = true
max_attempts = 3
max_duration = "30s"
delay_strategy = "EXPONENTIAL_BACKOFF"
backoff_initial_delay = "200ms"
backoff_max_delay = "5s"
backoff_multiplier = 2.0
jitter_factor = 0.1
method_policy = "IDEMPOTENT_ONLY"
```

固定 delay 示例：

```toml
[http.retry]
enabled = true
max_attempts = 2
delay_strategy = "FIXED"
fixed_delay = "500ms"
method_policy = "ALL_METHODS"
```

## 11. 测试计划

### 11.1 `qubit-retry`

新增测试：

1. `test_run_async_error_policy_success_first_attempt`
2. `test_run_async_error_policy_retries_until_success`
3. `test_run_async_error_policy_non_retryable_error_aborts`
4. `test_run_async_error_policy_max_attempts_returns_last_error`
5. `test_run_async_error_policy_max_duration_returns_last_error_when_present`
6. `test_run_async_error_policy_does_not_require_result_eq_hash`

其中第 6 个测试应使用一个不实现 `Eq + Hash` 的成功类型。

### 11.2 `qubit-http`

新增测试：

1. `GET` 第一次返回 `500`，第二次返回 `200`，最终成功。
2. `GET` 返回 `400`，不重试。
3. `GET` 连续返回 `503`，达到最大次数后返回最后一次 `HttpError::Status`。
4. `POST` 默认遇到 `500` 不重试。
5. `POST` 在 `method_policy = AllMethods` 时允许重试。
6. transport / write timeout 按 `RetryHint::Retryable` 重试。
7. `execute_stream()` 初始 `503` 可重试成功。
8. `execute_stream()` 返回成功后，body stream 中途 read timeout 不自动重试。
9. header injector 每次 attempt 都执行。
10. request-level headers 覆盖 client default headers 的语义在 retry 后保持不变。

测试服务器方面，现有 `one_shot_server` 只能处理单个请求。retry 集成测试需要新增一个 multi-shot 测试服务器，支持按顺序返回多个 `ResponsePlan`，并记录每次请求。

## 12. 兼容性与迁移

1. 默认 `retry.enabled = false`，现有调用方行为不变。
2. 新增 options 字段会影响直接构造 `HttpClientOptions` 的调用方。如果该 struct 是公开字段形式，调用方用 struct literal 构造时需要补 `retry` 字段。
3. 为降低破坏面，可以提供 builder/setter，或者在 release note 中明确说明改动。
4. `HttpClientOptions::default()` 和 `HttpClientOptions::new()` 不受影响。
5. 错误返回类型仍为 `HttpResult<T>`，不向 `qubit-http` 调用方暴露 `RetryRunError`。

## 13. 实施步骤

建议按以下顺序实施：

1. 在 `qubit-retry` 中新增 `RetryRunError<E>` 和 `run_async_error_policy()`。
2. 为 `qubit-retry` 补齐新增 runner 的单元测试。
3. 在 `qubit-http` 中新增 `HttpRetryOptions` 和 `HttpRetryMethodPolicy`。
4. 扩展 `HttpClientOptions::default()`、`from_config()` 和 `validate()`。
5. 将 `HttpClient::execute()` 拆为 `execute_once()` + `execute_with_retry()`。
6. 将 `HttpClient::execute_stream()` 拆为 `execute_stream_once()` + `execute_stream_with_retry()`。
7. 新增 multi-shot 测试服务器。
8. 补齐 HTTP retry 集成测试。
9. 更新 README 和既有设计文档中“只提供 retry hint”的描述。

## 14. 待确认问题

1. `retry.enabled` 默认是否保持 `false`。推荐保持 `false`，避免行为突变。
2. `HttpClientOptions` 是否接受新增公开字段带来的 struct literal 破坏。若不接受，需要先引入更稳定的 builder 或非公开字段策略。
3. `qubit-retry` 的新增 runner 是否放在 `RetryExecutor<(), C>` 上，还是新增单独类型 `ErrorPolicyRetryExecutor<C>`。推荐先放在 `RetryExecutor<(), C>`，改动更小。
4. `RetryRunError::MaxAttemptsExceeded` 是否应该保留 retry 框架错误本身作为 source。推荐不强行塞入 `HttpError`，由 `qubit-http` 选择如何记录 retry 上下文。
5. 是否需要 request-level retry override。推荐第一阶段不做。
