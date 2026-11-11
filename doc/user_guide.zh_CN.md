# qubit-http 用户指南

[English](user_guide.en.md) | [README](../README.zh_CN.md) | [API 文档](https://docs.rs/qubit-http)

本文档基于当前源码和测试整理，适用于 crate `qubit-http` 0.16.0，Rust 代码中通过库名 `qubit_http` 使用。

`qubit-http` 是一个异步 HTTP 客户端基础设施库。它封装 `reqwest`，提供统一的客户端配置、请求构建、响应读取、错误分类、TRACE 日志脱敏、自动重试、代理、IPv4-only 解析、请求/响应拦截器，以及 Server-Sent Events（SSE）解码和重连能力。

## 手册目标与读者

本文面向需要让多个 API 客户端共享请求、超时、重试、日志和响应处理
规则的 Rust 服务开发者，适用于 `qubit-http` 0.16.0 及公开的
`qubit_http` API。手册不替代上游服务的 API 契约，也不会替调用方决定
认证方案或请求是否可以安全重放。

## 如何阅读本文

| 目标 | 建议阅读 |
| --- | --- |
| 第一次接入 | 「快速开始」「构建请求」「读取响应」 |
| 配置客户端 | 「创建客户端」「从 qubit-config 读取」「配置参考」 |
| 排查失败 | 「错误模型」「自动重试」「日志脱敏」 |
| 使用流式响应或 SSE | 「读取响应」「SSE 解码」 |

## 概念模型

`HttpClient` 持有跨请求共享的执行策略；`HttpRequest` 组合方法、URL、请求头、请求体和请求级覆盖项。`execute` 应用客户端策略，并且只在状态码为 2xx 时返回 `HttpResponse`；拿到响应后，可以先检查元数据，再选择响应体读取方式；已缓存的响应仍可重复读取，未缓存的后端流只能移交一次。响应返回前发生的可重试失败属于内置重试边界；响应返回后读取流时出现的错误则由调用方处理。

## 安装与导入

<!-- guide-dependencies:start -->
```toml
[dependencies]
qubit-http = "^0.16"
qubit-redact = "^0.10"
http = "1.4"
qubit-config = { version = "^0.15", default-features = false }
qubit-retry = "^0.26"
qubit-budget = { version = "^0.10", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "sync"] }
futures-util = "0.3"
bytes = "1.11"
```
<!-- guide-dependencies:end -->

```rust
use http::Method;
use qubit_http::{HttpClient, HttpClientOptions};
```

## 快速开始

这段完整客户端程序会调用 HTTP API、附加客户端标识并读取响应体。运行前，
请启动支持 `GET /anything` 并返回 UTF-8 响应正文的测试服务。将服务基础 URL
设置到 `HTTP_EXAMPLE_BASE_URL`，再运行
`HTTP_EXAMPLE_BASE_URL=http://127.0.0.1:8080 cargo run`。该命令不会启动服务。

<!-- guide-check:quick-start:start -->
```rust
use http::Method;
use qubit_http::HttpClient;
use qubit_http::HttpClientOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base_url = std::env::var("HTTP_EXAMPLE_BASE_URL")?;
    let mut options = HttpClientOptions::new();
    options.set_base_url(&base_url)?;
    options.add_header("x-client-id", "demo")?;
    let client = HttpClient::builder().options(options).build()?;
    let request = client
        .request(Method::GET, "/anything")
        .query_param("from", "local-quick-start")
        .build();
    let mut response = client.execute(request).await?;
    println!("status = {}", response.status());
    println!("text = {}", response.text().await?);
    Ok(())
}
```
<!-- guide-check:quick-start:end -->

程序仅在收到 2xx 响应时成功，并输出该响应的状态码和正文；具体内容取决于服务。
若环境变量缺失、连接失败、响应非 2xx 或正文解码失败，程序会返回错误。无需外部服务的
本地运行方式是 `cargo run --example local_quick_start`；该示例会启动本地服务、检查响应，
并输出 `status = 200 OK` 和 `text = ready`。完整源码见
[`examples/local_quick_start.rs`](../examples/local_quick_start.rs)，无需外部服务或凭据。

`execute` 只把 2xx 状态码视为成功。非 2xx 状态会返回 `HttpErrorKind::Status`，并在错误中携带状态码、方法、URL、截断后的响应体预览，以及可解析的 `Retry-After`。

默认选项使用 `HttpClient::builder().build()`，显式选项使用 `HttpClient::builder().options(options).build()`，配置读取使用 `HttpClient::from_config(config)`。旧的 `HttpClientBuilder::create`、`create_default` 和 `create_from_config` 仍保留，但已弃用。特别是，`.options(saved).create_default()` 会忽略 `saved`：若要应用这些选项，改用 `.options(saved).build()`；若要使用默认值，则新建 `HttpClient::builder().build()`。

## 创建客户端

### 默认客户端

```rust
let client = qubit_http::HttpClient::builder().build()?;
```

默认行为：

| 项目 | 默认值 |
| --- | --- |
| `base_url` | 无；可以直接使用显式绝对 URL，也可以设置 base URL 后使用相对路径 |
| `origin_policy` | 默认 `SameOrigin`。初始 URL 按请求快照校验；重定向按执行客户端后端策略校验。详见[跨客户端来源策略](#跨客户端来源策略)。 |
| 连接超时 | 10 秒 |
| 读超时 | 120 秒 |
| 响应头／发送阶段超时 | 120 秒 |
| reqwest 单次网络请求及响应体读取期限 | 无 |
| 代理 | 显式代理禁用，且 `use_env_proxy = false`，因此调用 `reqwest` 的 `no_proxy()`，不会继承环境代理 |
| 日志 | 开启，但只有 tracing TRACE 级别启用时才输出 |
| 日志体预览 | 16 KiB |
| 非成功响应体预览 | 16 KiB |
| 整体响应体聚合（`response_body_size_limit`） | 8 MiB |
| 非成功响应原始 body 采集（`error_response_body_limit`） | 64 KiB |
| 自动重试 | 禁用 |
| 重试最大尝试次数 | 3，含第一次请求 |
| 重试方法策略 | 只允许幂等方法 |
| 日志脱敏 | 内置一组常见认证/密钥类 header、query 参数和 body 字段名 |
| IPv4-only | 关闭 |
| SSE JSON 模式 | `Lenient` |
| SSE 完成标记策略 | `DefaultDone`，即识别 `[DONE]` |
| SSE 单行上限 | 64 KiB |
| SSE 单帧上限 | 1 MiB |

### 跨客户端来源策略

请求构建时会捕获来源客户端的 base URL 和来源策略。调用 `execute` 的
客户端提供网络后端，其中包括重定向策略：

| 阶段 | 策略归属 |
| --- | --- |
| 初始 URL 解析与来源校验 | 构建请求的客户端快照 |
| 重定向来源校验 | 执行客户端的后端 |

例如，客户端 A 用 `SameOrigin` 构建请求，客户端 B 用 `AnyOrigin` 执行。
初始 URL 仍按 A 的快照校验，重定向则遵循 B 的策略。如果 B 使用
`SameOrigin`，即使 A 使用 `AnyOrigin`，B 的重定向策略仍会限制重定向。
重建客户端不会重绑已有请求；若两个阶段都要使用新客户端配置，请用该配置重新构建请求。

请求快照选择 `AnyOrigin` 时，初始目标可以跨源；执行后端选择
`AnyOrigin` 时，重定向也可以跨源。日志脱敏设置和
`HeaderValue::set_sensitive(true)` 只影响诊断输出，不会过滤网络请求头。
因此，允许跨源跳转时，自定义 API key header 可能继续发送；reqwest 只会
移除其固定名单中的标准认证头。携带凭证的请求应保留 `SameOrigin`。确需
跨源访问时，请使用不携带这些凭证的独立客户端/请求，并由应用校验目标。

### 使用代码配置

```rust
use std::time::Duration;
use qubit_http::{HttpClient, HttpClientOptions, HttpRetryMethodPolicy};
use qubit_retry::BackoffPolicy;

let mut options = HttpClientOptions::new();
options.set_base_url("https://api.example.com")?;
options.user_agent = Some("my-service/1.0".to_string());
options.max_redirects = Some(5);
options.timeouts.connect_timeout = Duration::from_secs(3);
options.timeouts.request_timeout = Some(Duration::from_secs(30));
options.retry.enabled = true;
options.retry.max_attempts = 4;
options.retry.backoff = BackoffPolicy::exponential(
    Duration::from_millis(100),
    2.0,
    Duration::from_secs(2),
)?;
options.retry.method_policy = HttpRetryMethodPolicy::IdempotentOnly;

let client = HttpClient::builder().options(options).build()?;
```

`create` 会先执行校验。常见校验包括：超时必须大于 0；`response_header_timeout` 还必须能表示为当前平台 `Instant` 的截止时间；启用代理时必须有非空 host 和非 0 port；只有设置 username 时才能设置 password；日志记录请求体或响应体时 `body_size_limit` 必须大于 0；`retry.max_attempts` 必须大于 0；`error_response_preview_limit` 必须大于 0；`user_agent` 不能为空并且必须是合法 header value；SSE 行/帧上限必须大于 0。backoff 与 jitter 在构造 `BackoffPolicy` 时完成校验。

如果已注册请求头注入器或拦截器，修改选项时应从现有客户端重建：

```rust
let mut updated = client.options().clone();
updated.timeouts.request_timeout = Some(std::time::Duration::from_secs(20));
let rebuilt = client.rebuild_with_options(updated)?;
```

`rebuild_with_options` 会创建新的后端连接池，按原顺序保留四类运行时注册项；构建失败时原客户端不变。`client.to_builder()` 只复制选项，之后调用 `.build()` 使用复制的选项；运行时注册项不会被复制。

```rust
let rebuilt_from_copied_options = client.to_builder().build()?;
```

### 额外 TLS 信任根

`HttpClientOptions::extra_root_certificates_der` 允许调用方把 DER 编码证书
追加到所选 TLS 后端使用的信任根中。每个 `Bytes` 值必须只包含一张 DER
证书。列表只会追加：后端默认信任来源会保留，不会被替换；空列表表示不
增加证书。这是代码配置，不新增 `qubit-config` 配置键，库也不会自动读取
证书文件。文件读取与 I/O 错误由调用方负责：

<!-- guide-check:tls-roots:start -->
```rust
let mut options = qubit_http::HttpClientOptions::new();
let certificate_der = std::fs::read("service-ca.der")?;
options.extra_root_certificates_der.push(bytes::Bytes::from(certificate_der));
let client = qubit_http::HttpClient::builder().options(options).build()?;
let _ = client;
```
<!-- guide-check:tls-roots:end -->

所选 TLS 后端提供默认信任根（例如平台或内置证书库）；额外证书会与其合并。
无效 DER 会在 `build()` 构造客户端时报告。如果所选 TLS 后端或平台不能使用
额外根证书，客户端构造会失败，不会静默忽略。若重建时选项仍保留这些证书，
新客户端也会继续使用它们；清空列表后重建即可恢复为后端默认信任来源：

```rust
let mut updated = client.options().clone();
updated.extra_root_certificates_der.clear();
let rebuilt = client.rebuild_with_options(updated)?;
```

### 从 qubit-config 读取

`HttpClientOptions::from_config` 和 `HttpClient::from_config` 接收任意 `qubit_config::ConfigReader`。如果传入 `config.section("http")`，下面表格中的键都按相对路径读取。

<!-- guide-check:configuration:start -->
```rust
use std::time::Duration;
use qubit_config::Config;
use qubit_http::HttpClient;

let mut config = Config::new();
config.set("http.base_url", "https://api.example.com".to_string())?;
config.set("http.timeouts.connect_timeout", Duration::from_secs(3))?;
config.set("http.retry.enabled", true)?;
config.set("http.retry.delay_strategy", "FIXED".to_string())?;
config.set("http.retry.fixed_delay", Duration::from_millis(250))?;

let client = HttpClient::from_config(&config.section("http")?)?;
```
<!-- guide-check:configuration:end -->

`HttpClient::from_config` 是推荐的配置入口。已弃用的 `create_from_config` 仍为兼容性保留，并忽略 builder 之前保存的选项。

`from_config` 会拒绝所选组件范围内的未知字段。例如
`http.timeouts.connect_timout` 会报错，而不会悄悄使用默认超时。独立的
`HttpTimeoutOptions`、`ProxyOptions`、`HttpLoggingOptions`、`HttpRetryOptions`
入口也检查各自字段，并在构造时返回已有领域校验失败。转换错误和领域错误都报告
根相对路径，例如 `service.http.timeouts.connect_timeout`。

`default_headers.*` 是开放的 header map，允许自定义 header 名。共享配置请通过
`config.section("http")?` 隔离；其他 section 不受检查。`${shared.host}` 一类插值
辅助值应放在根级 `shared.*` 等组件范围之外，仍可通过根回退解析。此前被忽略的
旧字段（例如根级 `sensitive_headers`）现在会报错，请迁移到已声明的字段。
由 rs-config source 加载的转换错误仍通过 `Error::source()` 保留其最终来源。

常用配置键：

| 键 | 说明 |
| --- | --- |
| `base_url` | 相对请求路径的基础 URL |
| `origin_policy` | `same_origin`（默认）按请求快照校验初始 URL；重定向使用执行客户端后端策略。`any_origin` 允许对应策略阶段访问跨源 URL；详见[跨客户端来源策略](#跨客户端来源策略) |
| `timeouts.connect_timeout` | 连接超时 |
| `timeouts.read_timeout` | 限制每次正常响应体读取及完整的非成功状态错误体采集阶段 |
| `timeouts.response_header_timeout` | 从异步准备到收到响应头的累计期限；同步请求拦截器在该期限开始前运行 |
| `timeouts.request_timeout` | 可选的 reqwest 单次网络请求及响应体读取期限；不包含异步 header 注入、流式 body 工厂准备、重试等待及其他尝试 |
| `proxy.enabled` | 是否启用代理 |
| `use_env_proxy` | 显式代理禁用时，是否继承环境变量代理 |
| `logging.enabled` | 是否允许 TRACE HTTP 日志 |
| `json.max_depth` | response、SSE 和配置 header JSON 的最大嵌套深度 |
| `json.max_nodes` | 单个 JSON value 的最大节点总数 |
| `json.max_output_bytes` | JSON 和 NDJSON 请求 body 编码后的累计最大字节数 |
| `log_redaction.url_path_policy` | URL path 策略：默认 `preserve`，也可显式设为 `redact` |
| `log_redaction.sensitive_headers` | 追加到默认集合的敏感 header 名称 |
| `log_redaction.sensitive_query_params` | 追加到默认集合的敏感 query 参数名称 |
| `log_redaction.sensitive_body_fields` | 追加到默认集合的敏感 JSON/form/multipart body 字段名称 |
| `log_redaction.excluded_sensitive_headers` | 显式允许保持可见的精确 header 名称 |
| `log_redaction.excluded_sensitive_query_params` | 显式允许保持可见的精确 query 参数名称 |
| `log_redaction.excluded_sensitive_body_fields` | 显式允许保持可见的精确结构化 body 字段名称 |
| `retry.enabled` | 是否启用内置重试 |
| `retry.max_attempts` | 最大尝试次数，含第一次请求 |
| `retry.delay_strategy` | `NONE`、`FIXED`、`RANDOM`、`EXPONENTIAL_BACKOFF` 或 `EXPONENTIAL` |
| `retry.method_policy` | `IDEMPOTENT_ONLY`/`IDEMPOTENT`、`ALL_METHODS`/`ALL`、`NONE`/`DISABLED` |
| `retry.status_codes` | 重试状态码白名单；未配置时默认重试 429 和 5xx |
| `retry.error_kinds` | 非状态错误类型白名单；未配置时默认重试超时和 transport |
| `sse.json_mode` | `LENIENT` 或 `STRICT` |
| `sse.done_marker` | `DISABLED`（或 `DISABLE`）禁用完成标记；`DEFAULT` 使用 `[DONE]`；其它非空字符串视为自定义完成标记（`Custom`），与 trim 后的 `data:` 文本比较 |
| `sse.max_line_bytes` | SSE 单行字节上限 |
| `sse.max_frame_bytes` | 两个空行边界间非空、非注释逻辑行的 UTF-8 累计字节上限；未知字段计入，结束符和注释不计入。注释行仍受 `sse.max_line_bytes` 限制；该项不代表线上帧大小或整个会话限额。 |

完整配置键见文末「配置参考」。

`default_headers` 支持两种形式，但不能同时使用。header 名称不区分大小写，因此两种形式中只差大小写的名称（例如 `Authorization` 与 `authorization`）都会被拒绝，不会互相覆盖。子键形式如下：

```rust
config.set("http.default_headers.authorization", "Bearer token".to_string())?;
config.set("http.default_headers.x-request-id", "abc-123".to_string())?;
```

也可以在 `default_headers` 写 JSON map 字符串；若同时配置 JSON map 和 `default_headers.*` 子键，解析会失败：

```rust
config.set(
    "http.default_headers",
    r#"{"x-app-id":"demo","x-version":"1.0"}"#.to_string(),
)?;
```

## 构建请求

通过 `client.request(method, path)` 创建 `HttpRequestBuilder`。`path` 可以是绝对 URL，也可以是相对路径。默认 `SameOrigin` 策略下，未配置 `base_url` 时允许显式绝对 URL，并把它作为本次请求的初始源；配置了 `base_url` 时，目标必须与它的 scheme、host 和有效端口一致。相对路径必须有 `base_url`。若要访问跨源初始目标，必须显式选择 `AnyOrigin`。初始 URL 按构建请求的客户端快照校验；重定向按执行客户端的策略校验。详见[跨客户端来源策略](#跨客户端来源策略)。

请求 URL 以及客户端或请求级 `base_url` 必须使用 `http` 或 `https`。`ftp`、`file`、`data` 等不支持的 scheme 会在请求解析时以 `InvalidUrl` 拒绝；客户端配置中的不支持 `base_url` 会作为 `base_url` 配置错误返回，避免被误分类为可重试的传输错误。

```rust
let request = client
    .request(Method::POST, "/events")
    .query_params([("source", "mobile"), ("debug", "false")])
    .header("x-request-id", "req-001")?
    .json_body(&serde_json::json!({"name": "created"}))?
    .request_timeout(Duration::from_secs(10))?
    .read_timeout(Duration::from_secs(30))?
    .build();
```

`build()` 后可以调用 `HttpRequest::resolved_url()` 取得真实发送 URL：绝对 `path` 会保持绝对 URL；相对路径会和 `base_url` join；`path` 中已有的 query 会保留，builder 或拦截器追加的 query 会按顺序继续追加。

相对路径遵循 `Url::join` 的规则：以 `/` 开头的路径从主机根目录开始；`base_url` 若没有尾斜杠，最后一段会被当作文件名并替换。要保留 `/v1/` 这样的 API 前缀，应给 `base_url` 加尾斜杠，并使用不以 `/` 开头的路径。

| `base_url` | `path` | 解析后的 URL |
| --- | --- | --- |
| `https://api.example.com/v1/` | `events` | `https://api.example.com/v1/events` |
| `https://api.example.com/v1/` | `/events` | `https://api.example.com/events` |
| `https://api.example.com/v1` | `events` | `https://api.example.com/events` |
| `https://api.example.com/v1` | `/events` | `https://api.example.com/events` |

下面的完整示例逐一检查四种组合解析出的 URL，不会发送请求：

<!-- guide-check:url-join:start -->
```rust
use http::Method;
use qubit_http::{HttpClient, HttpClientOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for (base, path, expected) in [
        ("https://api.example.com/v1/", "events", "https://api.example.com/v1/events"),
        ("https://api.example.com/v1/", "/events", "https://api.example.com/events"),
        ("https://api.example.com/v1", "events", "https://api.example.com/events"),
        ("https://api.example.com/v1", "/events", "https://api.example.com/events"),
    ] {
        let mut options = HttpClientOptions::default();
        options.set_base_url(base)?;
        let client = HttpClient::builder().options(options).build()?;
        assert_eq!(client.request(Method::GET, path).build().resolved_url()?.as_str(), expected);
    }
    Ok(())
}
```
<!-- guide-check:url-join:end -->

```rust
let request = client
    .request(Method::GET, "/users?active=true")
    .query_param("expand", "profile")
    .build();

let url = request.resolved_url()?;
assert_eq!(
    url.as_str(),
    "https://api.example.com/users?active=true&expand=profile",
);
```

请求体构建方法：

| 方法 | 行为 |
| --- | --- |
| `bytes_body` | 原始字节体，不推断媒体类型；会移除之前由 builder 自动生成的 `Content-Type` |
| `stream_body` | 调用时会把有限迭代器完整收集到内存，再通过 reqwest streaming body 按顺序发送；内存占用随总载荷增长。虽然已收集到内存，reqwest 仍将其视为流式 body，无法在内部 307/308 重定向中重放；会移除之前由 builder 自动生成的 `Content-Type` |
| `streaming_body` | 设置异步工厂，每次外层发送尝试生成新的字节流；内部 307/308 重定向不会再次调用工厂，流式 body 不会自动跟随此类重定向；会移除之前由 builder 自动生成的 `Content-Type` |
| `try_streaming_body` | 与 `streaming_body` 相同，但异步准备工厂返回 `HttpResult`，因此准备错误保留类型和 source；会移除之前由 builder 自动生成的 `Content-Type` |
| `text_body` | 文本体；设置或更新 builder 自动生成的 `text/plain; charset=utf-8` |
| `json_body` | 序列化 JSON；设置或更新 builder 自动生成的 `application/json` |
| `json_body_with_limits` | 使用本请求的 value 与输出字节上限序列化 JSON |
| `form_body` | 表单体；设置或更新 builder 自动生成的 `application/x-www-form-urlencoded` |
| `multipart_body` | 原始 multipart 字节；需要 1 到 70 个 token-safe 字符的 boundary；自动生成的 multipart 值会随新 boundary 更新，显式 multipart 值缺少 boundary 时会补齐；重复、格式错误或不匹配的 boundary 会被拒绝 |
| `ndjson_body` | 每条记录一行 JSON；设置或更新 builder 自动生成的 `application/x-ndjson` |
| `ndjson_body_with_limits` | 使用一个累计输出预算序列化 NDJSON，且包含行结束符 |

带媒体类型的 body helper 互相替换时，会更新前一个 helper 自动生成的 `Content-Type`。`bytes_body`、`stream_body`、`streaming_body` 和 `try_streaming_body` 不推断媒体类型，因此会清除 builder 自动生成的值。通过 `header` 或 `headers` 传入的请求级值始终视为显式设置，即使它和自动值相同也会保留；可以在 body helper 前或后设置。清除自动生成的请求级值后，客户端默认 header 或注入器可能重新生效，builder 不会清除这些来源。

如果旧链式调用依赖前一个 body helper 留下的媒体类型，应显式设置它：

```rust
let request = client.request(Method::POST, "/items")
    .header("content-type", "text/plain; charset=utf-8")?
    .text_body("old")
    .json_body(&serde_json::json!({"ready": true}))?
    .build();
assert_eq!(request.headers().get("content-type").unwrap(), "text/plain; charset=utf-8");
```

升级到包含这项行为变化的版本时，请检查连续调用多个 body helper 的链式代码。若前一个正文的媒体类型仍是业务要求，就显式调用 `header("content-type", ...)`；否则，最终的带媒体类型 helper 会设置自己的值，原始字节或流式 helper 则不会留下请求级自动值。

`HttpRequest::set_body` 和 `set_streaming_body` 是底层替换接口，会保留现有 header，也不会推断 `Content-Type`。媒体类型变化时，应配套调用 `set_header` 或 `remove_header`。

请求级覆盖：

| 方法 | 用途 |
| --- | --- |
| `request_timeout` | 可选的 reqwest 单次网络请求及响应体读取期限；不包含异步 header 注入、流式 body 工厂准备、重试等待及其他尝试 |
| `response_header_timeout` | 覆盖从异步准备到收到响应头的累计期限；同步请求拦截器在该期限开始前运行 |
| `read_timeout` | 限制每次正常响应体读取及完整的非成功错误体采集；超时后保留 status 和标记为不完整的部分 raw body |
| `base_url` / `clear_base_url` | 覆盖或清除本次请求 base URL |
| `ipv4_only` | 覆盖本次请求的 IPv4-only URL 校验 |
| `cancellation_token` | 绑定 `CancellationToken`，发送前、发送中、读 body/stream 时都会检查 |
| `force_retry` | 本次请求强制启用重试 |
| `disable_retry` | 本次请求禁用重试 |
| `retry_method_policy` | 覆盖本次请求的可重试 HTTP 方法策略 |
| `honor_retry_after` | 本次请求在 429/5xx 且可重试时尊重 `Retry-After` |

`request_timeout` 不限制异步 header 注入或流式 body 工厂准备。请求初始值会从创建它的客户端快照读取；值为 `None` 时使用执行客户端后端的默认值，调用 `clear_request_timeout()` 也会恢复此行为。同步请求拦截器运行后，`response_header_timeout` 才开始计时，并覆盖异步准备直到收到响应头。`read_timeout` 限制每次正常响应体读取，也限制完整采集非成功响应的错误体；超时时，返回的 `Status` 错误仍保留 status 和标记为不完整的部分 raw body。若必须保留慢错误体，请检查 `is_truncated()` 并提高 `read_timeout`。请求级 timeout 覆盖会拒绝零时长，并返回 `HttpError`。

## Header、注入器和拦截器

最终请求头合并顺序如下，后者覆盖同名 header：

1. 创建 builder 时快照下来的客户端默认 header。
2. 同步 `HttpHeaderInjector`，按注册顺序执行。
3. 异步 `AsyncHttpHeaderInjector`，按注册顺序执行。
4. 请求本身设置的 header。

<!-- guide-check:async-upload:start -->
```rust
use http::{HeaderMap, HeaderName, HeaderValue};
use qubit_http::{AsyncHttpHeaderInjector, HttpHeaderInjector};

client.add_header("x-client", "default")?;

client.add_header_injector(HttpHeaderInjector::new(|headers: &mut HeaderMap| {
    headers.insert(
        HeaderName::from_static("x-sync-token"),
        HeaderValue::from_static("sync-value"),
    );
    Ok(())
}));

client.add_async_header_injector(AsyncHttpHeaderInjector::new(|headers| {
    Box::pin(async move {
        headers.insert(
            HeaderName::from_static("x-async-token"),
            HeaderValue::from_static("async-value"),
        );
        Ok(())
    })
}));

let upload = client
    .request(http::Method::POST, "https://api.example.com/upload")
    .try_streaming_body(|| {
        Box::pin(async move {
            // 在这里打开或准备上传；`?` 会将准备错误直接返回给本次请求尝试。
            Ok(Box::pin(futures_util::stream::iter(vec![Ok(bytes::Bytes::from_static(b"chunk"))]))
                as qubit_http::HttpRequestBodyByteStream)
        })
    })
    .build();
let _ = upload;
```
<!-- guide-check:async-upload:end -->

`try_streaming_body` 会在每次外层发送尝试时执行工厂。打开或准备上传时发生的错误会直接返回，并保留 `HttpError` 类型和 source；字节流开始发送后产生的错误则沿用 reqwest 发送错误映射。准备阶段受 `response_header_timeout` 和请求取消 token 限制，但不受 `request_timeout` 限制。现有 `streaming_body` 调用无需迁移；需要在准备过程中使用 `?` 或保留特定错误类型时再迁移。重试仍按已配置的错误分类执行，可能再次调用工厂。内部 307/308 重定向不会重放流式 body。

异步注入器可以在发送前动态获取请求头，例如认证 token。此完整示例会复用有效缓存，并在 token 过期时刷新。启用自动重试时，注入器会在每次发送尝试前运行。多个请求同时遇到缓存失效时会等待刷新锁，并在锁内再次检查缓存。刷新失败不会写入缓存，因此后续调用可以重试。一小时有效期仅为示例值；实际应用应使用认证服务返回的有效期。取消会释放本地锁，但无法撤销认证服务已经执行的刷新，因此本示例不保证远端副作用恰好执行一次。

<!-- guide-check:token-refresh:start -->
```rust
use std::sync::Arc;
use std::time::{Duration, Instant};

use http::header::{AUTHORIZATION, HeaderValue};
use qubit_http::{AsyncHttpHeaderInjector, HttpClient, HttpError, HttpResult};
use tokio::sync::{Mutex, RwLock};

struct CachedToken {
    value: String,
    expires_at: Instant,
}

struct TokenProvider {
    cached: RwLock<Option<CachedToken>>,
    refresh_lock: Mutex<()>,
}

impl TokenProvider {
    fn new() -> Self {
        Self {
            cached: RwLock::new(None),
            refresh_lock: Mutex::new(()),
        }
    }

    async fn bearer_token(&self) -> HttpResult<String> {
        {
            let cached = self.cached.read().await;
            if let Some(token) = cached.as_ref() {
                if token.expires_at > Instant::now() + Duration::from_secs(30) {
                    return Ok(token.value.clone());
                }
            }
        }
        let _refresh_guard = self.refresh_lock.lock().await;
        {
            let cached = self.cached.read().await;
            if let Some(token) = cached.as_ref() {
                if token.expires_at > Instant::now() + Duration::from_secs(30) {
                    return Ok(token.value.clone());
                }
            }
        }
        let value = refresh_access_token().await?;
        let expires_at = Instant::now() + Duration::from_secs(3600);
        *self.cached.write().await = Some(CachedToken {
            value: value.clone(),
            expires_at,
        });
        Ok(value)
    }
}

async fn refresh_access_token() -> HttpResult<String> {
    // 调用认证服务、读取安全存储或执行其它异步刷新逻辑。
    Ok("fresh-token".to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = HttpClient::builder().build()?;
    let provider = Arc::new(TokenProvider::new());
    client.add_async_header_injector(AsyncHttpHeaderInjector::new(move |headers| {
        let provider = Arc::clone(&provider);
        Box::pin(async move {
            let token = provider.bearer_token().await?;
            let value = HeaderValue::from_str(&format!("Bearer {token}")).map_err(|error| {
                HttpError::other(format!("invalid authorization header: {error}"))
            })?;
            headers.insert(AUTHORIZATION, value);
            Ok(())
        })
    }));
    Ok(())
}
```
<!-- guide-check:token-refresh:end -->

请求拦截器在每次尝试发送前执行，可以修改 `HttpRequest`，返回错误会短路本次执行。响应拦截器只在成功状态响应上执行，接收 `HttpResponseInterceptorContext`：status 和请求方法不可变，响应 headers 和最终响应 URL 可以修改。返回错误会让 `execute` 失败。

```rust
use http::{HeaderName, HeaderValue};
use qubit_http::{HttpRequestInterceptor, HttpResponseInterceptor, HttpError};

client.add_request_interceptor(HttpRequestInterceptor::new(|request| {
    request.add_query_param("from_interceptor", "true");
    request.set_typed_header(
        HeaderName::from_static("x-intercepted"),
        HeaderValue::from_static("yes"),
    );
    Ok(())
}));

client.add_response_interceptor(HttpResponseInterceptor::new(|context| {
    if !context.headers().contains_key("x-required") {
        return Err(HttpError::other("missing x-required response header"));
    }
    Ok(())
}));
```

## 读取响应

`HttpResponse` 暴露 `meta()`、`status()`、`headers()`、`url()`、`request_url()`、`is_success()`、`retry_after_hint()` 以及读 body / 解码 SSE 的 API。`client.execute(...).await?` 得到 `mut response` 后，按下表选择响应体读取方式。未缓存的后端 body 只能移交给流一次；成功缓存后可以重复读取或创建流。表格下方 **「使用示例」** 小节给出表中各方法的典型写法。

body 可用方式：

| 方法 | 行为 |
| --- | --- |
| `bytes()` | 懒读取完整 body（首次 `await` 时读入并缓存），后续可重复调用 |
| `text()` | 基于 `bytes()`，用 UTF-8 解码完整 body |
| `json<T>()` | 基于 `bytes()`，用 serde JSON 反序列化完整 body |
| `stream()` | 返回 `HttpByteStream`；若 body 已缓存则为单块内存流，即使请求已取消也可读取。未缓存时调用本身**不会**立刻读完整 body，而是把底层响应交给返回的流，**在轮询该流时**按需读取字节块 |
| `sse_messages()` | 消费 `self`，按当前 SSE 行/帧上限等选项将 body 解码为 SSE 消息流（详见下文「SSE 解码」） |
| `sse_chunks::<T>()` | 消费 `self`，将 SSE `data:` JSON chunk 解码为 `SseChunk<T>` 流（JSON 模式、完成标记等见下文「SSE JSON chunk」） |

`bytes()`、`text()` 和 `json()` 都会聚合完整响应体，共用默认 8 MiB 的
`response_body_size_limit`。超过上限会返回
`HttpErrorKind::ResponseBodyTooLarge`。JSON helper 还受独立的 JSON value
和 payload 限额约束。若可信服务的响应大小已知，可在构建客户端时设置合适的
聚合预算；若下载较大或大小难以预计，应在开始读取前选择 `stream()`，逐块处理。
流式读取不受整包聚合上限约束，但应用仍应限制自行累计的字节数、存储空间和处理量。

### 使用示例

**整包读取（`bytes` / `text` / `json`）**：三者都会把完整 body 读入并缓存；缓存可供整包读取和 `stream()` 复用，成功缓存后仍可创建流。下面的 `response` 来自本节已有的 `client.execute(...).await?`，并在同一个异步函数内连续使用：

```rust
use futures_util::StreamExt;
use http::Method;
use serde::Deserialize;

#[derive(Deserialize)]
struct VersionInfo {
    version: String,
}

async fn read_whole_body_examples(
    client: &qubit_http::HttpClient,
) -> qubit_http::HttpResult<()> {
    let mut response = client
        .execute(client.request(Method::GET, "/bytes").build())
        .await?;
    let expected = response.bytes().await?;
    let mut cached_stream = response.stream()?;
    let chunk = cached_stream.next().await.transpose()?;
    assert_eq!(chunk.as_ref(), Some(&expected));
    assert!(cached_stream.next().await.is_none());
    assert_eq!(response.bytes().await?, expected);

    let mut r2 = client
        .execute(client.request(Method::GET, "/hello.txt").build())
        .await?;
    let _text = r2.text().await?;

    let mut r3 = client
        .execute(client.request(Method::GET, "/version.json").build())
        .await?;
    let _json: VersionInfo = r3.json().await?;
    Ok(())
}
```

**流式字节（`stream`）**：对未缓存的 body，`stream()` 会把后端 body 移交给返回的流，之后无法再取回以进行整包读取；`?` 在拿到流时展开，例如请求上的取消 token 已在开始读取 body 前触发时会在这里返回错误。取消或已有的 body 读取错误仍会返回错误，不能通过其他读取方式绕过。整包读取成功后，即使请求随后取消，也可以读取缓存或解码缓存的 SSE；仍需后端 I/O 的读取会被取消阻止。如果应用要求 token 取消后立即停止业务处理，应在应用处理边界检查 token。以下独立请求示例展示未缓冲的网络流式读取，不是缓存内存示例。

```rust
use futures_util::StreamExt;
use http::Method;

async fn read_stream_example(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let mut response = client
        .execute(client.request(Method::GET, "/stream-bytes").build())
        .await?;

    let mut stream = response.stream()?;
    while let Some(item) = stream.next().await {
        let chunk = item?;
        let _len = chunk.len();
    }
    Ok(())
}
```

逐块处理后按应用策略释放或存储数据；若把所有块重新放进一个不设上限的缓冲区，
仍会带来整包聚合的内存风险。整包读取返回 `ResponseBodyTooLarge` 后，该响应的读取
已经失败，不能再对同一个 `HttpResponse` 切换到 `stream()` 恢复内容。若业务需要重新
发请求，应由应用判断操作能否安全重放；响应体读取失败不会触发客户端自动重试。

在发送请求前，也可以为预期较大的响应提高聚合预算。下面示例设置 16 MiB，属于应用
自己的预算，并非库的默认值；JSON value 和 payload 限额仍独立生效：

```rust
use qubit_http::{HttpClient, HttpClientOptions};

fn client_for_known_large_responses() -> qubit_http::HttpResult<HttpClient> {
    let options = HttpClientOptions {
        response_body_size_limit: 16 * 1024 * 1024,
        ..HttpClientOptions::default()
    };
    HttpClient::builder().options(options).build()
}
```

**SSE（`sse_messages` / `sse_chunks`）**：二者都会**消费** `self`（转移所有权），内部仍基于同一条 body 流；若要在解码前调整行/帧上限、JSON 模式、完成标记等，可将各 `sse_*` 配置方法与 `sse_messages()` / `sse_chunks::<T>()` 写在同一条链上（完整说明见下文「SSE 解码」「SSE JSON chunk」）。

```rust
use futures_util::StreamExt;
use http::Method;
use qubit_http::sse::SseChunk;

#[derive(Debug, serde::Deserialize)]
struct Delta {
    text: String,
}

async fn read_sse_examples(client: &qubit_http::HttpClient) -> qubit_http::HttpResult<()> {
    let mut ev = client
        .execute(client.request(Method::GET, "/events").build())
        .await?;
    let mut events = ev.sse_messages();
    while let Some(item) = events.next().await {
        let _event = item?;
    }

    let mut jc = client
        .execute(client.request(Method::GET, "/chat-stream").build())
        .await?;
    let mut chunks = jc.sse_chunks::<Delta>();
    while let Some(item) = chunks.next().await {
        match item? {
            SseChunk::Data(_d) => {}
            SseChunk::Done => break,
        }
    }
    Ok(())
}
```

成功调用 `bytes`、`text` 或 `json` 后，缓存可供整包读取和新的单块 `stream()` 复用，即使请求之后取消也一样。未缓存的后端 body 会移交给流，之后无法再取回以进行整包读取；取消或已有的 body 读取错误仍会返回错误。`sse_messages` / `sse_chunks` 也基于流读取，并按值消费 `HttpResponse`；已缓存的 SSE body 在取消后仍可解码。

未缓冲响应调用 `bytes`、`text` 或 `json` 时，聚合读取 future 首次被轮询后会接管后端 body。若外层 `timeout` 或 `select!` 落败分支随后丢弃该 future，而缓存尚未完成，后端响应和已读字节都会丢失；后续读取返回 `ResponseBodyAlreadyConsumed`。尚未轮询的 future 被丢弃不会接管 body，已成功缓存的数据仍可复用。这与库内读取器返回的取消或读取超时错误不同，后者会保留原有失败分类。需要得到分类明确的读取失败时，应在发送请求前配置 read timeout 或取消令牌；外部取消未缓冲读取后，应将该响应视为不可再读。

| 可以 | 避免 |
| --- | --- |
| `bytes()` / `text()` / `json()` 读取完整响应体并复用缓存 | 先 `stream()`，再对同一响应调用 `bytes()` / `text()` / `json()` |
| `bytes()` 后调用 `stream()`；缓存仍可供后续读取或再次创建流 | 调用 `sse_messages()` 或 `sse_chunks()` 后继续使用已移动的响应值 |
| 在同一条表达式中链式设置 `sse_*` 选项并消费 SSE | 对同一个 `HttpResponse` 同时设计多条消费路径 |

metadata 的状态为 429 或 5xx 且带有合法 `Retry-After` header 时，`retry_after_hint()` 会返回延迟。它支持 `delta-seconds` 和 HTTP-date 两种格式；过去的日期返回 0 秒。`execute` 会在成功响应拦截器运行前将非成功状态映射为 `HttpError`，因此应从错误读取 429/5xx 提示：

```rust,ignore
match client.execute(request).await {
    Ok(response) => handle_response(response).await?,
    Err(error) => {
        if let Some(delay) = error.retry_after {
            schedule_retry(delay);
        } else {
            return Err(error.into());
        }
    }
}
```

`HttpResponseMeta` 和 `HttpResponseInterceptorContext` 仍保留 `retry_after_hint()`，供成功拦截器之外检查 metadata 或读取手动构造的 metadata。

## 错误模型

所有运行时 HTTP 错误使用 `HttpError`，结果别名是 `HttpResult<T> = Result<T, HttpError>`。

`HttpError` 包含：

| 字段 | 含义 |
| --- | --- |
| `kind` | 错误分类 |
| `method` | 可选 HTTP 方法 |
| `url` | 可选请求 URL |
| `status` | 可选响应状态码 |
| `message` | 人类可读错误信息 |
| `response_body_preview` | 非 2xx 响应体预览 |
| `retry_after` | 从 `Retry-After` 解析出的延迟 |
| `source` | 底层错误 |

错误分类：

非 2xx 错误的脱敏预览和保留原始 body 使用独立限额。预览可能已截断而 raw body 仍完整。`HttpStatusResponse::is_truncated()` 保持兼容语义：raw 字节有遗漏或采集未能确认完整时为 true，但它不说明具体原因。使用 `capture_end()` 区分 `Complete`、`SizeLimit`、`ReadTimeout`、`ReadFailure` 和 `BodyUnavailable`；该枚举为 non-exhaustive，匹配时需提供兜底分支。仅 `ReadFailure` 的 `capture_error()` 会返回 backend 错误及其 source 链；记录前须脱敏。该查询独立于顶层 `HttpError::source()` 重试错误链，并会随状态快照经过重试投影保留。只有 `capture_end()` 为 `Complete` 时才解析 provider 字段；仅 preview 截断不表示 raw body 不完整。默认最多保留 64 KiB 原始字节；raw body 可能含敏感信息，不应直接写入日志，解析前先确认采集完整。

| 分组 | 错误类型 | 典型含义 |
| --- | --- | --- |
| URL / 配置 | `InvalidUrl`, `BuildClient`, `ProxyConfig` | URL 无法解析、客户端构建失败或代理配置非法 |
| 超时 / 网络 | `ConnectTimeout`, `ReadTimeout`, `ResponseHeaderTimeout`, `RequestTimeout`, `Transport` | `ConnectTimeout` 表示后端报告的连接阶段超时；其它发送阶段超时仍为 `RequestTimeout`，非超时连接失败仍为 `Transport` |
| HTTP 状态 | `Status` | 收到非 2xx 状态码 |
| 解码 / SSE | `Decode`, `SseProtocol`, `SseDecode` | 响应体解码失败、SSE 协议错误或 SSE JSON chunk 解码失败 |
| 重试层 | `RetryBudgetExceeded` 与 `HttpRetryDiagnostics` | 重试预算或终止策略诊断 |
| 响应体 | `ResponseBodyTooLarge`, `ResponseBodyAlreadyConsumed` | 整包聚合超出配置上限，或响应体流已经被取走 |
| 取消 / 兜底 | `Cancelled`, `Other` | 请求取消，或无法归入其它分类的错误 |

超时类别按观察到失败的阶段划分，并不对应唯一的超时配置。收到响应头后
读取成功响应体时，即使到期的是 `request_timeout`，错误类别仍为
`ReadTimeout`，而非 `RequestTimeout`。仅凭 `ReadTimeout` 无法判断具体是哪项
超时配置到期。非成功响应的错误体采集遵循上文独立的 `Status` 和采集诊断规则。

普通 HTTP 自动重试与 SSE 自动重连有不同的终止错误契约：

| 路径 | 终止条件 | 返回结果与直接 source |
| --- | --- | --- |
| 普通 HTTP 自动重试 | 耗尽次数或时长预算时已有最后一次业务错误 | 保留该 HTTP kind，附加 `HttpRetryDiagnostics`，并将完整 `RetryError<HttpError>` 保留为 source |
| 普通 HTTP 自动重试 | 捕获业务错误前预算已耗尽 | 可返回 `RetryBudgetExceeded` 并保留 retry 错误；取消等终态仍按各自映射处理 |
| SSE 重连 | 存在可重试错误但次数已耗尽 | 返回最后的 HTTP/SSE 错误并保持其原有 source；不承诺普通 HTTP 的 retry diagnostics |
| SSE 重连 | operation/total 时间预算耗尽 | 返回 `RetryBudgetExceeded`，保留已有请求上下文；若有最后一次 `HttpError`，直接以该错误作为 source |
| SSE 重连 | 预算记账自身失败 | 保持现有 `Other` kind 和 `RetryBudgetError` source |
| SSE 重连 | 清洁 EOF 耗尽次数，或收到 204 | 正常结束流 |

SSE 不承诺普通 HTTP 自动重试的直接 source 类型或诊断信息。

`retry_metadata()` 是已弃用的兼容 getter，始终返回 `None`，不能用来判断错误是否经过普通 HTTP 重试。需要该路径的终态信息时使用 `retry_diagnostics()`，并从 `Error::source()` 检查 `RetryError<HttpError>`。SSE 解码错误会在原错误未提供相应字段时补上响应 method、原始请求 URL 和 status；URL 格式化仍使用响应对应的脱敏策略。

`retry_hint()` 会把超时、transport、429 和 5xx 状态视为可重试提示，其余默认不可重试。新增的重试层错误分类本身也不可重试。真正是否重试还要结合 `HttpRetryOptions` 和方法策略。

## 排障

先查看错误的 `kind`、`message`、`status`、`url` 和 `source`。下面按可观察
现象列出常见的配置或用法问题：

| 现象 | 检查项 | 处理方式 |
| --- | --- | --- |
| 相对请求路径返回 `InvalidUrl` | 是否设置了 `base_url` | 配置 client/request 级 base URL，或改用绝对 URL。 |
| 跨 origin 的 URL 或重定向被拒绝 | `origin_policy` 与 base URL | 默认 `SameOrigin` 用于隔离；只有服务契约确实需要跨 origin 时才显式使用 `AnyOrigin`。请求由一个客户端构建、另一个客户端执行时，详见[跨客户端来源策略](#跨客户端来源策略)。 |
| 请求没有重试 | `retry.enabled`、`max_attempts`、方法策略和错误白名单 | 开启重试，并确认当前方法及错误/状态码允许且适合重放。 |
| 流式读取后再次读取 body 失败 | 是否已移交未缓存的后端流 | 已缓存响应仍可重复读取；未缓存后端流移交后无法再取回。 |
| 没有日志或日志出现敏感值 | `logging.enabled`、TRACE subscriber 等级和脱敏策略 | 同时开启日志条件，并在创建 client 前配置 `RedactionPolicy`。 |
| SSE 重连拒绝响应 | `Content-Type` 和取消状态 | 自动重连要求 `text/event-stream`；取消和 SSE 协议错误默认不会触发重连。 |

配置失败时，向 `from_config` 传入作用域化的 `config.section("http")`，
可保留根路径上下文。重试失败时，检查 `retry_diagnostics()`，并沿
`Error::source()` 找到保留的 `RetryError<HttpError>`。
非 2xx body 不完整时，可检查 `status_response().capture_end()`，区分大小限额、采集超时、
backend 读取失败和流不可用。backend 读取失败时可用 `capture_error()` 查询来源；记录前须
脱敏。只有结束原因是 `Complete` 时才将保留 body 解析为 provider JSON。

### SSE 重连取消归属

原始请求取消 token 属于整个重连调用，可中断重连等待并阻止后续尝试。request interceptor 可以为当前尝试替换独立 token，控制本次发送和响应体读取，也可以移除本次尝试 token；这些操作不会改变原始重连级取消来源。

## 自动重试

自动重试默认关闭。开启后，只有当 `retry.enabled = true`、`max_attempts > 1`，并且方法策略允许当前 HTTP 方法时，`execute` 才会进入重试流程。

`max_attempts` 和重试诊断统计本库的外层尝试次数。一次 reqwest 执行可能包含安全协议恢复和重定向；两者都不会创建新的外层尝试。关闭本库重试只会停止这些外层重试，仍保留 reqwest 的安全协议恢复。请求拦截器和动态请求头注入按外层尝试运行，reqwest 内部恢复不会重新运行它们。`request_timeout` 限制一次 reqwest 执行及其响应体读取，不会为每次底层网络发送重置。请求构建时还会捕获来源客户端的 base URL、origin policy、默认请求头和注入器；执行客户端提供后端、执行拦截器和重试策略。客户端重建不会重绑旧请求；若要使用新配置，应通过重建后的客户端创建新请求。初始 URL 按请求快照检查，重定向按执行后端的重定向策略检查。

默认重试条件：

| 类型 | 默认可重试 |
| --- | --- |
| 状态码 | 429 和所有 5xx |
| 非状态错误 | connect/read/response-header/request timeout 和 transport |
| 方法 | GET、HEAD、PUT、DELETE、OPTIONS、TRACE |

可以用 `retry.status_codes` 和 `retry.error_kinds` 配置白名单。白名单一旦设置，就只重试列出的状态码或错误类型。

`retry.error_kinds` 可使用所有 `HttpErrorKind` 名称的配置形式。配置值会 trim，`read-timeout` 一类连字符形式会归一化为 `read_timeout`；请使用小写 snake_case 或等价的连字符形式。

request interceptor 可以在重试流程准入后修改 method 或重试覆盖项；最终 method 和覆盖项决定该流程能否继续。拦截器不能启动此前未准入的重试流程。拦截器顺序见 [Header、注入器和拦截器](#header-注入器和拦截器)。

对单个请求可以覆盖：

```rust
let request = client
    .request(Method::POST, "/submit")
    .force_retry()
    .retry_method_policy(qubit_http::HttpRetryMethodPolicy::AllMethods)
    .honor_retry_after(true)
    .build();
```

`honor_retry_after(true)` 只在请求级启用。遇到可重试的 429 或 5xx 时，它会把响应中的 `Retry-After` 值作为提示交给退避策略。默认 HTTP 退避策略将提示作为最小延迟，下一次尝试等待计划退避和提示中的较大值。自定义 `BackoffPolicy` 可以改变这一行为：`ignore_retry_after()` 会忽略提示，`limit_delay(duration)` 则限制最终延迟，即使提示更长也会受此上限约束。SSE 重连也采用最终请求偏好；开启后，可重连的 429/5xx 握手错误会把解析出的提示交给重连退避策略。`server_retry_max_delay` 只限制 SSE `retry:` 字段；HTTP 提示仍受用户配置的退避上限和重连预算约束。

开启重试后，`execute` 会把每次尝试交给 `qubit-retry` 的 `Retry`。HTTP `max_duration` 使用单调时钟统计。可重试错误耗尽 `max_attempts` 或 `max_duration` 后仍返回最后一次 HTTP 错误并附加 `HttpRetryDiagnostics`，完整 `RetryError<HttpError>` 作为 `source` 保留。

这是**软性续试预算**：它可以拒绝计划中的等待或下一次准入，但不会取消正在执行的请求，也不会覆盖成功结果。
普通 HTTP 重试没有设置 `qubit-retry` 的 `attempt_timeout` 或 `flow_timeout`。例如，
`max_duration = 1s` 的已准入请求仍可能耗时 2s 后成功；`request_timeout` 仅限制 reqwest 单次网络执行和响应体读取，不包含异步 header 注入、流式 body 工厂准备、重试等待及其他尝试；异步准备到响应头使用 `response_header_timeout`。`read_timeout` 也限制完整的非成功响应错误体采集；慢错误体超时后可检查 `is_truncated()`，并在需要时提高 `read_timeout`。返回后才惰性读取的响应体仍不在内置重试范围内。

可重试请求必须能够安全重放。缓冲请求体可重新发送，`streaming_body` 工厂则必须为每次外层尝试创建新流；
请求头注入器和拦截器也可能重复执行。两类流式请求体都不会通过 reqwest 内部 307/308 重定向重放；
如果没有外层重试策略再次重试该状态，`execute` 会返回对应的 `Status` 错误。优先使用最终上传 URL；
载荷符合应用内存预算时，可改用 `bytes_body`。在默认 HTTP 退避策略下，`honor_retry_after` 将提示作为最小延迟；
提示过长时可能直接耗尽续试预算，而不再发送请求。自定义提示策略或最终延迟上限可以改变选中的等待时间。
SSE 重连使用独立的重放策略，详见 [SSE 自动重连](#sse-自动重连)。

当前版本使用 `qubit-retry` 0.26。应用若与 HTTP/SSE 共享 `RetryPolicy` 或 `BackoffPolicy`，
须同步升级直接依赖及锁文件。直接消费 retry 结果时，可通过 `RetryError::map_error` 做纯业务载荷转换，
保留重试上下文和完成诊断；最终 `HttpError` 仍遵循 HTTP 自身的领域转换规则，不改为通用重试错误 API。
直接消费 retry 结果时，应读取 `completion_callback_failures()` 或调用
无损的 `into_parts()` 三元组；旧名称 `into_parts_with_diagnostics()` 已删除。
穷举匹配 `RetryCallbackPhase` 时还需处理 `Success` 和 `TerminalFailure`。

| 场景 | 返回错误 | 说明 |
| --- | --- | --- |
| 方法策略不允许重放，例如默认策略下的 POST | 原始单次执行错误 | 不进入重试流程 |
| 已进入重试流程，但当前错误不可重试 | 原始 `HttpError` + `Aborted` 诊断 | `source` 保留完整 `RetryError<HttpError>` |
| 可重试，但耗尽 `max_attempts` | 最后一次 `HttpError` | `message` 会追加 attempts exhausted 上下文 |
| 可重试，但耗尽 `max_duration` | 最后一次 `HttpError` 或 `RetryBudgetExceeded` | 已捕获错误时返回最后一次错误；尚未捕获时返回 `RetryBudgetExceeded` |

读取重试终态和底层上下文时，使用受支持的诊断信息及 source 链。`retry_metadata()` 仅为源码兼容保留，现已弃用并始终返回 `None`。

```rust
use std::error::Error;
use qubit_http::HttpError;
use qubit_retry::RetryError;

fn inspect_retry(error: &HttpError) {
    if let Some(diagnostics) = error.retry_diagnostics() {
        eprintln!("{diagnostics:?}");
    }
    if let Some(retry) = error
        .source()
        .and_then(|source| source.downcast_ref::<RetryError<HttpError>>())
    {
        eprintln!("reason={:?}, attempts={}", retry.reason(), retry.context().attempts());
    }
}
```

## 日志脱敏

HTTP 日志使用 `tracing::trace!`。必须同时满足：

1. `options.logging.enabled = true`。
2. 当前 tracing subscriber 开启 TRACE 级别。

一次 HTTP 请求执行统一使用 `HttpRequest` 捕获的 `Redactor` 处理请求/响应日志、执行错误、返回后的响应读取及 SSE 诊断。执行客户端控制日志开关和展示选项。客户端 A 构建的请求由客户端 B 执行时仍使用 A 的策略，客户端重建也不会重绑旧请求；需要新策略时，请用相应客户端重新构建请求。A/B 策略不会合并。独立构造的 `HttpLogger` 仍使用构造时显式传入的策略。

可分别控制请求头、请求体、响应头、响应体。body 只记录前 `logging.body_size_limit` 字节，超出部分显示截断提示；二进制体显示为 `<binary N bytes>`。没有结构化或文本 `Content-Type` 的 unsupported body 会显示为 `<redacted: unsupported HTTP body>`，不会直接打印原始内容。请求体日志只预览已缓冲的 body 变体（`bytes_body`、`text_body`、`json_body`、`form_body`、`multipart_body`、`ndjson_body`）；`stream_body` 和 `streaming_body` 会记录为 `<skipped: streaming request body>`，因为 logger 不会消费上传流。

日志统一经过规范的 `qubit_redact::Redactor` 和一份根 `RedactionPolicy` 脱敏。URL username、password、fragment 和敏感 query 参数会被掩码；header、query/form、body 使用同一基础规则、掩码表和静态限制上的 HTTP 上下文视图。multipart 文件 part 仍显示为 `<redacted: file part>`；格式异常、缺少 boundary 或已截断的 body 继续 fail closed。

对于 header，`http::HeaderValue::is_sensitive()` 是值级 `Secret` 声明。请求、响应、流式响应和 `Debug` 渲染都会在 header name 匹配前处理该标记。allow 规则不能暴露已标记值；未标记值继续使用同一个不可变 name policy 快照。

`RedactionPolicy::builder()` 使用空应用规则和标准 floor；扩展默认快照时使用 `RedactionPolicy::default().to_builder()`。应用 allow 规则无法绕过启用的 floor；在分组 `http(|http| { ... })` 视图中调用 `http.disable_all_floors()` 是关闭 HTTP 上下文 floor 的显式逃生口。`RedactionPolicy` 和 `Redactor` 必须从 `qubit_redact` 导入，`qubit_http` 不会重新导出它们。`logging.body_size_limit` 是展示限额，结构化 body 解析由 JSON 与结构限额约束。

应用启动时使用 `Redactor::replace_application_default(Redactor::new(policy))` 安装默认 redactor；`RedactionPolicy::default()` 始终
返回固定标准策略。`HttpClientOptions::new()` 会取得构造时默认 redactor 的快照，包括此前已安装的应用策略。client 创建一份
`Redactor` 快照，由 request builder 捕获到请求中，并在请求执行、response、retry、interceptor、error 和 SSE 诊断中
沿用该请求策略。客户端 A 构建的请求即使由客户端 B 执行，请求/响应日志、执行错误、返回后的响应读取和 SSE 诊断仍使用 A 的策略；执行客户端控制后端、日志开关及展示选项、拦截器和重试策略，A/B 策略不合并。客户端重建不会替换旧请求策略，需要使用新策略时应重新构建请求。独立构造的 `HttpLogger` 继续使用构造时显式传入的策略。request builder 和 options 的前置错误使用所属对象保存的策略快照，
即使应用默认策略后来发生变化也如此。使用 `fields()`、`http()` 和 `limits()` 配置统一的
`RedactionPolicyBuilder`；`qubit_http` 不再重导出这些脱敏类型。

示例：

<!-- guide-check:redaction:start -->
```rust
use http::Method;
use qubit_http::{
    HttpClient,
    HttpClientOptions,
    HttpLogger,
};
use qubit_redact::{RedactionPolicy, Sensitivity};
use serde_json::json;

let mut options = HttpClientOptions::new();
options.logging.enabled = true;
options.logging.log_request_header = true;
options.logging.log_request_body = true;
options.logging.log_response_body = true;
let builder = RedactionPolicy::default().to_builder().http(|http| {
    let _ = http.header().raise("x-api-key", Sensitivity::High);
    let _ = http.query().raise("access_token", Sensitivity::High);
    let _ = http.body().raise("password", Sensitivity::Secret);
})?;
options.log_redaction_policy = builder.build()?;

let client = HttpClient::builder().options(options).build()?;
let request = client
    .request(Method::POST, "https://api.example.com/login")
    .query_param("access_token", "secret-token")
    .header("x-api-key", "secret-key")?
    .json_body(&json!({
        "username": "alice",
        "password": "secret-password",
    }))?
    .build();

let mut response = client.execute(request).await?;
let _ = response.bytes().await?;
let logger = HttpLogger::new(client.options());
logger.log_response(&mut response).await?;
```
<!-- guide-check:redaction:end -->

自动响应日志在 `execute` 返回前触发。如果此时响应体尚未消费，日志会显示 `<skipped: body not yet consumed>`；即使已知 `Content-Length`，logger 也不会为了日志读取响应体。之后读取响应体不会自动补记日志。若要记录已缓冲的响应体，应先完成缓冲读取，再显式调用上方的 `HttpLogger::log_response`。这次调用会再次记录响应行，以及已启用的响应头。除开启 logging 开关外，应用的 tracing subscriber 也必须允许 TRACE 事件。

显式记录仍遵循 body 展示限额和脱敏规则。如果 `bytes()` 读取失败，`?` 会返回错误，后续 logger 调用不会执行。缓冲读取也受配置的读取超时和取消 token 约束。此用法适用于能在 `response_body_size_limit` 内完整缓冲的有限响应体；不要为了日志而消费未缓存的 `stream()` 或 SSE 响应。请将示例中的 `https://api.example.com/login` 替换为你自己的服务地址；指南检查器只编译示例，不会连接该地址。示例由同一 client 构造并执行请求，logger 也从该 client 的 options 构造。独立创建的 logger 使用传给构造器的策略，不会自动继承另一份请求快照。

## 代理与 IPv4-only

代理默认禁用；同时 `use_env_proxy` 默认也是 `false`，因此客户端调用 `no_proxy()`，不会使用环境变量代理。若需要在未配置显式代理时继承 `HTTP_PROXY` / `HTTPS_PROXY` 等环境变量，可设置 `options.use_env_proxy = true` 或配置 `use_env_proxy = true`。启用显式代理时必须设置 host 和 port。

`proxy.host` 只接受域名、IPv4 字面量或完整 IPv6 字面量（可以带成对方括号）。端口填写在 `proxy.port`，凭据分别填写在 `username` / `password`；scheme、路径、query、userinfo 和 host 中的端口都会被拒绝。

```rust
use qubit_http::{ProxyType, HttpClientOptions};

let mut options = HttpClientOptions::new();
options.proxy.enabled = true;
options.proxy.proxy_type = ProxyType::Socks5;
options.proxy.host = Some("127.0.0.1".to_string());
options.proxy.port = Some(1080);
```

`ProxyType::Socks5` 使用 scheme `socks5h`，即远端 DNS。设置 username 后会启用代理 Basic Auth；只设置 password 不设置 username 会校验失败。

`ipv4_only = true` 时，本地 resolver 只保留 IPv4 解析结果；请求 URL 中的 IPv6 literal host、显式配置的 IPv6 literal 代理地址（在客户端构造时）以及现有 IPv6 重定向检查仍会被拒绝。环境代理的选择仍委托给 reqwest：客户端到 IPv6 环境代理的连接不受本地 resolver 限制，因此仍可能成功。`ProxyType::Socks5` 使用 `socks5h`，目标域名交给代理远端解析；HTTP 和 HTTPS 代理也自行控制目标解析与出口。本地 resolver 既无法限制客户端到代理这一段，也无法约束代理到目标这一段的地址族，因此 IPv4-only 不是跨代理链路的端到端保证。部署若要求代理出口为 IPv4，应由代理自身限制，或关闭环境代理并使用经过验证的直连配置。

## SSE 解码

先按目标选择 API：

| 目标 | 使用 |
| --- | --- |
| 读取原始 SSE message | `response.sse_messages()` |
| 读取 OpenAI 风格 JSON chunk 或 `[DONE]` 完成标记 | `response.sse_chunks::<T>()` |
| 长连接断开后自动重连 | `client.execute_sse_with_reconnect(...)` |

`sse_messages()` 和 `sse_chunks::<T>()` 只解码响应体，不检查 `Content-Type`。
如果上游协议要求 SSE，直接调用这两个方法前应确认响应声明了
`text/event-stream`。自动重连 API 会自行检查；媒体类型缺失或不合法时返回
`SseProtocol` 错误。媒体类型不匹配时使用固定摘要，不包含原始
`Content-Type`；调用方应匹配 `SseProtocol` 并检查 method、URL 和 status，
不要再匹配 message 中的 header 原值。

SSE 消息解码从 `HttpResponse` 开始：

```rust
use futures_util::StreamExt;
use http::Method;

let response = client
    .execute(client.request(Method::GET, "/stream").build())
    .await?;
response.validate_sse_content_type()?;

let mut messages = response.sse_messages();
while let Some(item) = messages.next().await {
    let message = item?;
    println!(
        "event={:?} last_event_id={:?} data={}",
        message.event, message.last_event_id, message.data
    );
}
```

### 配置 `sse_messages` 选项

`sse_max_line_bytes` 与 `sse_max_frame_bytes` 均返回 `HttpResponse`（按值移动后的 `self`），可在消费响应体之前与 `sse_messages()` 写在同一条链上，依次写入本响应上的 SSE 解析上限（`sse_messages` 会按此时的配置从 `stream()` 解码）：

```rust
use futures_util::StreamExt;
use http::Method;

let response = client
    .execute(client.request(Method::GET, "/stream").build())
    .await?;

let mut messages = response
    .sse_max_line_bytes(64 * 1024)      // 单行最大字节数
    .sse_max_frame_bytes(1024 * 1024) // 单帧最大字节数
    .sse_messages();
while let Some(item) = messages.next().await {
    let message = item?;
    println!(
        "event={:?} last_event_id={:?} data={}",
        message.event, message.last_event_id, message.data
    );
}
```

`SseMessage` 字段：

| 字段 | 含义 |
| --- | --- |
| `event` | `event:` 字段，可选 |
| `data` | 多行 `data:` 用 `\n` 拼接 |
| `last_event_id` | 应用当前帧以及之前 control-only `id:` 帧后的 EventSource last-event-id |

协议行为：

- 支持 CR、LF 和 CRLF 三种换行符。
- 每行必须是 UTF-8，否则返回 `HttpErrorKind::SseProtocol`。
- 只有空行会派发事件。UTF-8 BOM 仅在首行开头时忽略一个，正文其它位置的 BOM 会保留。
- 注释行（以 `:` 开头）忽略。
- 未知字段忽略。
- `retry:` 只有非空且完全由 ASCII 数字组成、并能解析为 `u64` 时才生效；其它值会被忽略，也不会覆盖此前有效的延迟提示。
- `retry:` 和 control-only `id:` 帧只作为内部控制记录使用，不会作为 `SseMessage` 暴露。
- EOF 不会派发未完成的数据事件；完整的 retry 和 control-only id 控制记录仍按既有规则处理。服务端必须用空行结束每个事件，包括最后一个事件；关闭连接不能代替数据帧终结。
- 单行和单帧上限默认来自 `HttpClientOptions`；若本次响应需要不同上限，在调用 `sse_messages` 之前将 `sse_max_line_bytes` / `sse_max_frame_bytes` 与 `sse_messages()` 链在同一条表达式上即可（完整示例见本节上文「配置 `sse_messages` 选项」）。

帧限额统计两个空行边界之间所有非空、非注释逻辑行的 UTF-8 累计字节数；字段名、分隔符、值和未知字段都计入。行结束符、空行分隔符和注释行不计入；注释行仍受 `max_line_bytes` 限制。首行 BOM 会在计数前移除。累计值等于上限时允许，超过上限返回 `SseProtocol`。该限额不代表线上完整帧大小、整个会话的字节数、事件数或 CPU 时间预算；行限额、读取超时和取消机制仍分别承担各自职责。

### SSE JSON chunk

以下代码片段假设已经构建好 `request`，并且 `MyChunk` 和 `handle` 已在调用方定义。

`sse_chunks` 无参数：完成标记策略默认为 `DoneMarkerPolicy::DefaultDone`（即 `DoneMarkerPolicy` 的 `Default` 实现，识别 trim 后等于 `[DONE]` 的 `data:`），并可通过 `HttpClientOptions::sse_done_marker_policy` 或响应上的 `sse_done_marker_policy` 覆盖。

JSON SSE 完成策略默认是 `SseCompletionPolicy::AllowEof`。设置
`SseCompletionPolicy::RequireDoneMarker` 后，完整事件中必须出现已配置的完成标记；没有标记时，
会在已解码数据之后产生一次 `SseProtocol` 错误。AllowEof 允许完整事件流在没有 marker 时结束，
但不会派发未完成的末帧。禁用完成标记或使用空白自定义标记时，该策略会被拒绝。

```rust
use futures_util::StreamExt;
use qubit_http::sse::SseChunk;

let response = client.execute(request).await?;
let mut chunks = response.sse_chunks::<MyChunk>();

while let Some(item) = chunks.next().await {
    match item? {
        SseChunk::Data(data) => handle(data),
        SseChunk::Done => break,
    }
}
```

### 配置 `sse_chunks` 选项

`json_value_limits`、`sse_json_mode`、`sse_done_marker_policy`、`sse_max_line_bytes`、`sse_max_frame_bytes` 均返回 `HttpResponse`（按值移动后的 `self`），可在调用 `sse_chunks::<T>()` 前链式覆盖本响应的 JSON value 预算、SSE JSON 模式、完成标记策略与行/帧上限：

```rust
use futures_util::StreamExt;
use qubit_budget::json::JsonValueLimits;
use qubit_http::sse::{DoneMarkerPolicy, SseChunk, SseJsonMode};

let response = client.execute(request).await?;

let mut chunks = response
    .json_value_limits(JsonValueLimits::builder().max_nodes(10_000).build())
    .sse_json_mode(SseJsonMode::Strict)
    .sse_done_marker_policy(DoneMarkerPolicy::DefaultDone)
    .sse_max_line_bytes(256)
    .sse_max_frame_bytes(16 * 1024)
    .sse_chunks::<MyChunk>();

while let Some(item) = chunks.next().await {
    match item? {
        SseChunk::Data(data) => handle(data),
        SseChunk::Done => break,
    }
}
```

`DoneMarkerPolicy` 支持：

- `Disabled`：不识别完成标记。
- `DefaultDone`：`data:` 去除空白后等于 `[DONE]` 时输出 `SseChunk::Done` 并结束。
- `Custom(String)`：使用自定义完成标记。

`SseJsonMode::Lenient` 会跳过 malformed JSON chunk 并继续。自动解码错误使用固定摘要，并通过 source 保留安全的 `JsonDecodeError`。Lenient DEBUG 日志只记录错误类型/阶段以及 event/id 是否存在和字节数，不记录原文；调用方仍可读取原始 message 字段，并应自行控制日志策略。 依赖旧错误文本或记录 event/id 原文的应用应改为检查 `HttpErrorKind` 并读取 `JsonDecodeError` source；只有在明确制定应用日志策略后才记录消息字段。`Strict` 会在第一个 malformed JSON 处返回 `HttpErrorKind::SseDecode`。JSON value 预算可通过 `HttpClientOptions::json_value_limits` 或 `json.max_*` 配置，SSE frame 限制通过 `sse.*` 配置。若只覆盖本次响应，可按上例把 `json_value_limits`、`sse_json_mode`、`sse_done_marker_policy`、`sse_max_line_bytes`、`sse_max_frame_bytes` 与 `sse_chunks::<T>()` 链在同一表达式中。

### SSE 自动重连

<!-- guide-check:sse-reconnect:start -->
```rust
use futures_util::StreamExt;
use http::Method;
use qubit_http::sse::SseReconnectOptions;
use qubit_retry::{BackoffPolicy, RetryPolicy};

let request = client.request(Method::GET, "/events").build();
let mut events = client.execute_sse_with_reconnect(
    request,
    SseReconnectOptions {
        retry: RetryPolicy::builder()
            .max_attempts(6) // 1 次初始连接 + 5 次重连
            .backoff(BackoffPolicy::exponential(
                std::time::Duration::from_secs(1),
                2.0,
                std::time::Duration::from_secs(30),
            )?)
            .build()?,
        reconnect_on_eof: true,
        honor_server_retry: true,
        server_retry_max_delay: None,
        apply_jitter_to_server_retry: true,
    },
);

while let Some(item) = events.next().await {
    let event = item?;
    println!("{}", event.data);
}
```
<!-- guide-check:sse-reconnect:end -->

`RetryPolicy` 的准入预算覆盖以下阶段：

| 阶段 | operation 时间 | total 时间 |
| --- | --- | --- |
| 建立连接及校验响应 | 累计已完成尝试所耗费的时间 | 累计实际经过时间 |
| 读取 SSE record，包括消费者暂停轮询 | 不计入 | 累计实际经过时间 |
| 重连等待 | 不计入 | 累计实际经过时间 |

Operation 时间累计已完成连接建立及响应校验尝试所耗费的时间，不包括 SSE 消费或重连等待。Total 时间包括经过的消费和等待时间；准入限额不会中断正在读取的流。这些预算控制是否允许开始后续连接，并非正在读取流的硬截止时间。请求超时、读取超时和取消仍按各自规则独立生效。

默认重连配置为 `retry.max_attempts = 4`（即最多重连 3 次），使用 `initial=1s`、`max=30s`、`multiplier=2.0` 且无 jitter 的指数 `BackoffPolicy`，并在 EOF 后重连、尊重服务端 `retry:`。最新有效的 `retry:` 会持续生效，直到新有效值替换；可用 `honor_server_retry = false` 忽略服务端提示。服务端 `retry:` 会被限制到 `server_retry_max_delay`；若该字段为 `None`，则从 backoff policy 的最大值推导，无法推导时使用 30 秒。这个上限只作用于 SSE `retry:` 字段。可重连的 429/5xx 握手错误是否采用解析出的 HTTP 提示，由最终请求的 `honor_retry_after` 偏好决定；HTTP `Retry-After` 仍是只影响单次尝试的提示，遵循独立退避策略上限和重连预算。`apply_jitter_to_server_retry` 默认开启；只有传入的 `RetryPolicy` 配置了 jitter 时，服务端给出的延迟才会被扰动。重连时会复用原始请求，并禁用内层 HTTP 自动重试；收到非空 SSE `id:` 字段后（包括 control-only 控制记录中的 ID），下一次请求会带上 `Last-Event-ID` header；空 ID 重置后下一次请求会省略该头。204 响应会正常结束重连流，不要求 `Content-Type`。这些是有意的行为变化：调用方应将 204 的流结束视为正常停流，服务端若要更改延迟应发送新的有效 `retry:`。本 API 不宣称完整实现 EventSource。其他成功状态仍要求 `Content-Type` 为 `text/event-stream`。SSE 解码错误会补充响应 method、原始请求 URL 和 status，同时保留更具体的原有上下文与 source。取消错误不会触发重连；SSE 协议错误默认也不会重连；超时、transport、429/5xx 等 retryable 错误，以及包含 unexpected EOF 语义的错误可触发重连。

SSE 重连使用独立的 `SseReconnectOptions.retry` 策略。普通 HTTP 重试设置、`retry.method_policy` 以及请求级 `disable_retry()` 都不会禁止外层 SSE 重连。因此原请求可能被重复发送，包括 `POST` 或 `PATCH`。优先使用 `GET`；对有副作用的操作，应确认操作和请求体在业务幂等保证下可以安全重复。工厂能生成新的请求体流只代表技术上可重放，并不保证业务幂等。需要完全控制重放时，可分别通过 `execute` 管理每次连接。

服务端一旦提供事件 ID，最终的 `Last-Event-ID` 就由重连执行器管理。它会在默认 header、注入器和请求拦截器合并完成后写入非空游标；空 ID 则删除该 header。在服务端尚未提供 ID 前，仍按普通 header 优先级处理。若应用需要自行控制每次重连游标，应通过 `execute` 分别管理各次请求。

游标更新遵循解码器的提交边界。仅含控制字段的帧中，完整的 `id:` 行会在正常 EOF 或 body 读取失败/超时之前保留，因此下一次重连会使用该 ID；空值 `id:` 会清空游标并删除请求头。附带 data 的 ID 只有在空行终结并派发事件时才提交。如果流在此边界前失败，未完成 data 会被丢弃，其 ID 不会替换旧游标。正常 EOF 时最后一条未终止的 `id:` 残片仍可处理；读取错误截断的残片不是完整行，会被忽略。含 NUL 的 ID 会被忽略。

## 配置参考

下表列出 `HttpClientOptions::from_config` 支持的完整配置键。若传入 `config.section("http")`，这些键都按相对路径读取。

| 键 | 说明 |
| --- | --- |
| `base_url` | 相对请求路径的基础 URL |
| `ipv4_only` | 启用后 DNS 只保留 IPv4 地址，并拒绝 IPv6 literal URL |
| `error_response_preview_limit` | 非 2xx 错误中保留的响应体预览字节数；默认 16 KiB |
| `response_body_size_limit` | 整体响应体与 JSON helper 允许累计的最大字节数；默认 8 MiB |
| `error_response_body_limit` | 非 2xx `HttpStatusResponse` 保留的原始 body 最大字节数；默认 64 KiB，与默认 16 KiB 的脱敏 `error_response_preview_limit` 独立 |
| `user_agent` | 传给 `reqwest` builder 的默认 User-Agent |
| `max_redirects` | 最大重定向次数 |
| `pool_idle_timeout` | 连接池空闲超时 |
| `pool_max_idle_per_host` | 每个 host 最大空闲连接数 |
| `use_env_proxy` | 显式代理禁用时是否继承环境代理；默认 `false` |
| `log_redaction.url_path_policy` | URL path 策略：默认 `preserve`，也可显式设为 `redact` |
| `log_redaction.sensitive_headers` | 追加到默认敏感 header 集合的字符串列表 |
| `log_redaction.sensitive_query_params` | 追加到默认敏感 query 参数集合的字符串列表 |
| `log_redaction.sensitive_body_fields` | 追加到默认敏感 body 字段集合的字符串列表 |
| `log_redaction.excluded_sensitive_headers` | 显式允许保持可见的精确 header 名称字符串列表 |
| `log_redaction.excluded_sensitive_query_params` | 显式允许保持可见的精确 query 参数名称字符串列表 |
| `log_redaction.excluded_sensitive_body_fields` | 显式允许保持可见的精确结构化 body 字段名称字符串列表 |
| `origin_policy` | `same_origin`（默认）按请求快照校验初始 URL；重定向使用执行客户端后端策略。`any_origin` 允许对应策略阶段访问跨源 URL；详见[跨客户端来源策略](#跨客户端来源策略) |
| `default_headers` | 默认请求 header 的 JSON map 字符串；不能与 `default_headers.<name>` 同时使用；大小写无关的重复名称会被拒绝 |
| `default_headers.<name>` | 一个默认请求 header 子键；不能与 `default_headers` JSON map 同时使用；大小写无关的重复名称会被拒绝 |
| `json.max_depth` | JSON 最大嵌套深度；默认 `128` |
| `json.max_nodes` | JSON 最大节点总数；默认 `100000` |
| `json.max_sequence_items` | 单个 JSON array 的最大元素数；默认 `100000` |
| `json.max_map_entries` | 单个 JSON object 的最大条目数；默认 `100000` |
| `json.max_key_bytes` | 单个 object key 的最大 UTF-8 字节数；默认 `65536` |
| `json.max_string_bytes` | 单个 string 的最大 UTF-8 字节数；默认 `8388608` |
| `json.max_number_bytes` | 单个 number 词法表示的最大字节数；默认 `4096` |
| `json.max_payload_bytes` | key、string 与 number 的累计 payload 字节上限；默认 `8388608` |
| `json.max_output_bytes` | JSON/NDJSON 请求 body 编码后的累计最大字节数（含 NDJSON 行结束符）；默认 `8388608` |
| `timeouts.connect_timeout` | 连接超时 |
| `timeouts.read_timeout` | 限制每次正常响应体读取及完整的非成功状态错误体采集阶段 |
| `timeouts.response_header_timeout` | 从异步准备到收到响应头的累计期限；同步请求拦截器在该期限开始前运行 |
| `timeouts.request_timeout` | 可选的 reqwest 单次网络请求及响应体读取期限；不包含异步 header 注入、流式 body 工厂准备、重试等待及其他尝试 |
| `proxy.enabled` | 是否启用代理 |
| `proxy.proxy_type` | `http`、`https`、`socks5` 或 `socks5h` |
| `proxy.host` | 代理主机 |
| `proxy.port` | 代理端口 |
| `proxy.username` | 代理 Basic Auth 用户名 |
| `proxy.password` | 代理 Basic Auth 密码，必须配合 username |
| `logging.enabled` | 是否允许 TRACE HTTP 日志 |
| `logging.log_request_header` | 是否记录请求头 |
| `logging.log_request_body` | 是否记录请求体预览 |
| `logging.log_response_header` | 是否记录响应头 |
| `logging.log_response_body` | 是否记录响应体预览 |
| `logging.body_size_limit` | 日志体预览字节数 |
| `retry.enabled` | 是否启用内置重试 |
| `retry.max_attempts` | 最大尝试次数，含第一次请求 |
| `retry.max_duration` | 可选的软性续试预算，包含尝试、等待和重试控制回调 |
| `retry.delay_strategy` | `NONE`、`FIXED`、`RANDOM`、`EXPONENTIAL_BACKOFF` 或 `EXPONENTIAL` |
| `retry.fixed_delay` | 固定延迟 |
| `retry.random_min_delay` | 随机延迟下限 |
| `retry.random_max_delay` | 随机延迟上限 |
| `retry.backoff_initial_delay` | 指数退避初始延迟 |
| `retry.backoff_max_delay` | 指数退避最大延迟 |
| `retry.backoff_multiplier` | 指数退避倍率 |
| `retry.jitter_factor` | 抖动比例，范围 `0.0..=1.0` |
| `retry.method_policy` | `IDEMPOTENT_ONLY`/`IDEMPOTENT`、`ALL_METHODS`/`ALL`、`NONE`/`DISABLED` |
| `retry.status_codes` | 重试状态码白名单；未配置时默认重试 429 和 5xx |
| `retry.error_kinds` | 非状态错误类型白名单；未配置时默认重试超时和 transport |
| `sse.json_mode` | `LENIENT` 或 `STRICT` |
| `sse.done_marker` | `DISABLED`（或 `DISABLE`）禁用完成标记；`DEFAULT` 使用 `[DONE]`；其它非空字符串视为自定义完成标记（`Custom`），与 trim 后的 `data:` 文本比较 |
| `sse.completion` | `allow_eof`（默认）或 `require_done_marker`；后者要求启用且非空的 `sse.done_marker` |
| `sse.max_line_bytes` | SSE 单行字节上限 |
| `sse.max_frame_bytes` | 两个空行边界间非空、非注释逻辑行的 UTF-8 累计字节上限；未知字段计入，结束符和注释不计入。注释行仍受 `sse.max_line_bytes` 限制；该项不代表线上帧大小或整个会话限额。 |

## 限制与最佳实践

- 对只读或幂等接口可开启全局重试。对 POST/PATCH，必须同时启用重试并设置允许重放的方法策略，例如 `AllMethods`。请求级 `force_retry()` 只启用重试，不会覆盖方法策略或 `max_attempts`；只有业务允许重复执行时才应允许重放。
- 对长连接/SSE 设置合理的 `read_timeout`；过短会把正常的慢流误判为 `ReadTimeout`。
- 自动 TRACE 响应体日志在 `execute` 返回前触发，只记录当时已经缓存的 body。后续读取不会自动补记；需要时应在缓冲读取成功后显式调用 `HttpLogger::log_response`。这会再次记录响应行及已启用的响应头，也不会消费未缓存的 stream 或 SSE 响应。
- 需要完全禁用代理时保持默认 `proxy.enabled = false` 且 `use_env_proxy = false`；需要继承环境代理时显式打开 `use_env_proxy`。
- 如果使用 `from_config`，优先传入 `section("http")` 一类的作用域视图，这样错误路径会保留完整上下文。

本 crate 有意不暴露全部 `reqwest` API；`HttpResponse` 返回后发生的流式
读取错误也不会重新进入 HTTP 重试；TRACE 日志不会为了记录内容而消费
尚未缓存的响应体；之后读取也不会自动补记，需要显式调用
`HttpLogger::log_response`。整体 body 与 JSON helper 受配置的大小预算约束，SSE
解码还受单行和单帧上限约束。只有在业务允许重复执行时，才应把已缓冲的
请求体或每次都创建新流的 `streaming_body` 工厂视为可重放。

## 从 0.15 升级到 0.16

本节以仓库 `v0.15.1` 标签和提交 `e93a4cb` 中的 `0.16.0` 为比较依据。crate 声明的最低 Rust 版本仍为 1.94。请将依赖要求更新为 `qubit-http = "0.16"`；`^0.15` 不包含 `0.16`。

| 项目 | 已核实的变化或保留契约 | 调用方操作 |
| --- | --- | --- |
| 依赖 | manifest 将 `qubit-json` 从 0.10 更新到 0.12、`qubit-config` 从 0.14 更新到 0.15、`qubit-budget` 从 0.7 更新到 0.10、`qubit-redact` 从 0.9 更新到 0.10，Tokio 从 1.53 更新到 1.54。`reqwest` 仍为 0.13。 | 检查 lockfile 和直接依赖约束是否满足所选 `qubit-http` 版本。 |
| Base URL | 配置键仍为 `base_url`，现在仅接受 HTTP 或 HTTPS URL。没有新增替代键。 | 检查是否配置了 `file:`、`ws:` 等 scheme；改用 HTTP(S) API 基址。 |
| 配置 | 对比源码没有发现 `retry`、`timeouts` 或 `sse` 配置键改名。当前指南列出支持的键和值。 | 用 `HttpClientOptions::from_config` 或 `HttpClient::from_config(...)` 解析实际配置；将未知键或无效值错误作为迁移项处理。 |
| 重试归属 | `execute` 只会在返回 `HttpResponse` 前重试符合条件的失败；返回后的响应体和 SSE 读取由调用方负责，不会重新进入 HTTP 重试。上层 SDK 可以拥有单独的操作级重试。 | 如果 SDK 已重试整个操作，保留请求级 `disable_retry()`，如 OpenAI provider 的做法。验证一次应用层尝试不会意外叠加多层重试。 |
| 客户端 hook | 两个版本的 `to_builder()` 都只复制选项；`rebuild_with_options()` 创建新 backend 并保留已注册的注入器和拦截器。 | 修改已配置客户端的选项时，若 hook 属于契约则使用 `rebuild_with_options()`；不要假定 `to_builder()` 会复制 hook。 |
| Options 结构体字面量 | `HttpClientOptions` 新增 `extra_root_certificates_der` 字段。 | 穷举字段的字面量需补上此字段；使用 `..HttpClientOptions::default()` 的字面量无需修改。 |
| SSE | 0.16 源码增加对空 `id:` 重置、有效 SSE `retry:` 提示持续生效，以及 `204 No Content` 在媒体类型检查前正常结束的处理。严格 JSON 与完成标记策略仍由调用方选择。 | 检查游标重置和重试时间预期。严格 JSON 流应显式使用 `SseJsonMode::Strict` 与 `SseCompletionPolicy::RequireDoneMarker`，并测试完整帧和未完成帧。 |

要从配置创建客户端，可以直接使用下面的完整示例。它只构建客户端，不会发送网络请求。支持的配置键和读取方式见[从 qubit-config 读取](#从-qubit-config-读取)。

```rust
use qubit_config::Config;
use qubit_http::HttpClient;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::new();
    let _client = HttpClient::from_config(&config)?;
    Ok(())
}
```

验证下游 SDK 升级时，请检查普通 JSON 响应解码、SDK 拥有重试时单次网络请求、严格 SSE 完成条件，以及 `rebuild_with_options()` 后 hook 保留情况。可以用独立的小型 Cargo consumer 覆盖这些调用方式；该检查不能证明所有下游包都已完成升级。


## 延伸阅读

- [README](../README.zh_CN.md) 和 [English README](../README.md)
- [English User Guide](user_guide.en.md)
- [设计文档](design.zh_CN.md) 和 [Design](design.md)
- [API 文档](https://docs.rs/qubit-http)
