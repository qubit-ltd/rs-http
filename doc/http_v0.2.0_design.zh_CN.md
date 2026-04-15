# `rust-http` `v0.2.0` 设计文档

## 文档信息

- 文档名称：`rust-http v0.2.0 设计文档`
- 文档版本：`v1.0`
- 创建日期：`2026-04-09`
- 当前版本：`0.1.0`
- 目标版本：`0.2.0`
- 前置假设：
  - `qubit-value v0.3.0` 已支持 `usize/isize`、`Duration`、`Url`、`HashMap<String, String>` 与 `Json`
  - `qubit-config v0.4.0` 已支持 prefix 子树提取、`Option<T>`、结构化反序列化与改进后的来源解析

## 1. 背景

`qubit-http v0.1.0` 已具备统一 HTTP 语义和强类型运行时 options，但仍缺少一条标准化的配置接入路径。

当前调用方如果想从配置文件或环境变量创建 `HttpClientOptions`，只能在业务层手工做以下工作：

1. 读取 dotted keys。
2. 手工解析 `Duration`、`Url`、enum。
3. 手工组装 `default_headers`、`sensitive_headers`。
4. 手工校验 proxy / timeout / logging 等字段。

这会让不同调用方再次分叉。

## 2. `v0.2.0` 目标

`v0.2.0` 的目标是为 `qubit-http` 增加一条标准、可选、低侵入的配置映射入口，但不改变当前强类型 options 作为运行时核心对象的设计。

也就是说：

1. `HttpClientOptions` 继续是运行时强类型配置对象。
2. `Config` 只作为输入来源，不作为底层存储模型。
3. 配置集成能力通过可选 feature 或适配层引入。

## 3. 核心设计结论

### 3.1 不把 `Config` 作为 `HttpClientOptions` 的底层实现

`HttpClientOptions`、`HttpTimeoutOptions`、`ProxyOptions`、`HttpLoggingOptions` 继续保留当前公开 struct 设计。

原因：

1. 运行时访问路径更短。
2. `factory` 和 `client` 不需要知道配置来源。
3. 保持当前 API 稳定。
4. 避免把配置容器语义渗透到请求执行层。

### 3.2 增加从 `Config` 到 options 的标准转换入口

`v0.2.0` 增加的能力是：

1. `Config -> HttpClientOptions`
2. `Config -> HttpTimeoutOptions`
3. `Config -> ProxyOptions`
4. `Config -> HttpLoggingOptions`

## 4. 建议新增功能

### 4.1 Cargo feature

建议新增可选 feature：

1. `config`

语义：

1. 默认不启用，保持 `qubit-http` 的基础依赖面尽量小。
2. 启用后才引入 `qubit-config` 适配能力。

### 4.2 转换 API

建议新增：

```rust
impl HttpClientOptions {
    pub fn from_config(config: &qubit_config::Config) -> Result<Self, HttpConfigError>;
    pub fn from_config_with_prefix(
        config: &qubit_config::Config,
        prefix: &str,
    ) -> Result<Self, HttpConfigError>;

    pub fn validate(&self) -> Result<(), HttpConfigError>;
}

impl HttpTimeoutOptions {
    pub fn from_config(config: &qubit_config::Config, prefix: &str)
        -> Result<Self, HttpConfigError>;
}

impl ProxyOptions {
    pub fn from_config(config: &qubit_config::Config, prefix: &str)
        -> Result<Self, HttpConfigError>;
}

impl HttpLoggingOptions {
    pub fn from_config(config: &qubit_config::Config, prefix: &str)
        -> Result<Self, HttpConfigError>;
}
```

同时保留工厂入口：

```rust
impl HttpClientFactory {
    pub fn create(&self) -> Result<HttpClient, HttpError>;
    pub fn create_with_options(
        &self,
        options: HttpClientOptions,
    ) -> Result<HttpClient, HttpError>;
    pub fn create_from_config(
        &self,
        config: &qubit_config::Config,
        prefix: &str,
    ) -> Result<HttpClient, HttpConfigError>;
}
```

## 5. 标准配置 schema

建议在 `v0.2.0` 中固定以下 schema：

```text
http.base_url
http.ipv4_only

http.timeouts.connect_timeout
http.timeouts.read_timeout
http.timeouts.write_timeout
http.timeouts.request_timeout

http.proxy.enabled
http.proxy.proxy_type
http.proxy.host
http.proxy.port
http.proxy.username
http.proxy.password

http.logging.enabled
http.logging.log_request_header
http.logging.log_request_body
http.logging.log_response_header
http.logging.log_response_body
http.logging.body_size_limit

http.default_headers.*
http.sensitive_headers
```

其中：

1. `base_url` 读取为 `Url`
2. `*_timeout` 读取为 `Duration`
3. `proxy_type` 读取为 enum
4. `body_size_limit` 读取为 `usize`
5. `default_headers` 先读取为 `HashMap<String, String>`，再在适配层转换为 `HeaderMap`
6. `sensitive_headers` 支持字符串列表

## 6. `default_headers` 的输入策略

`HeaderMap` 不进入 `qubit-value` 的原生值模型，因此 `qubit-http` 适配层需要支持两种输入：

1. 子键形式

```text
http.default_headers.Authorization = Bearer xxx
http.default_headers.X-Request-Id = abc
```

2. JSON 文本或结构化 map 形式

```json
{"Authorization":"Bearer xxx","X-Request-Id":"abc"}
```

结论：

1. `qubit-http` 不要求 `qubit-value` 支持 `HeaderMap`。
2. `HashMap<String, String>` 作为配置中间表示足够。
3. 对环境变量等纯文本来源，JSON 文本是合理补充。

## 7. 校验能力下沉

`v0.2.0` 需要把“配置合法性校验”从工厂内部部分抽离出来，形成显式校验层。

建议：

1. `HttpClientOptions::validate()`
2. `ProxyOptions::validate()`
3. `HttpLoggingOptions::validate()`

目标：

1. 在真正创建 `reqwest::Client` 前完成配置校验。
2. 让配置错误和运行时网络错误分层。
3. 便于测试与文档化。

## 8. 错误模型

建议新增独立错误类型（**当前实现**已落盘，与下列结构一致）：

```rust
pub enum HttpConfigErrorKind {
    MissingField,
    TypeError,
    InvalidValue,
    InvalidHeader,
    ConfigError,
}

pub struct HttpConfigError {
    pub path: String,
    pub message: String,
    pub kind: HttpConfigErrorKind,
}
```

源码：`src/http_config_error_kind.rs`、`src/http_config_error.rs`。

要求：

1. 明确指出失败配置路径，例如 `http.proxy.port`。
2. 区分“缺失配置”“类型错误”“值非法”“Header 转换失败”及底层 `qubit-config` 错误（见 `HttpConfigErrorKind`）。
3. 与现有 `HttpError` 分层，不把配置阶段错误混入请求执行错误。

## 9. 文档与示例

`v0.2.0` 需要补充以下文档能力：

1. TOML 示例
2. YAML 示例
3. 环境变量示例
4. `Config -> HttpClientOptions` 示例
5. `default_headers` 使用子键和 JSON 两种方式的示例

## 10. 非目标

以下内容不属于 `v0.2.0` 范围：

1. 把 `HttpClientOptions` 改造成 `Config` 的包装器
2. 在 `qubit-value` 中原生支持 `HeaderMap`
3. 把任意业务配置逻辑塞回 `qubit-http`

## 11. 验收标准

1. 在启用 `config` feature 后，可从 `Config` 直接创建 `HttpClientOptions`。
2. 支持 prefix 读取，例如 `http.*`。
3. 支持 `Duration`、`Url`、enum、`usize`、`HashMap<String, String>` 的配置映射。
4. 支持把 `HashMap<String, String>` 转换为 `HeaderMap`。
5. 配置错误返回 `HttpConfigError`，并携带明确路径。
6. 不启用 `config` feature 时，现有 `v0.1.0` API 和依赖面保持基本不变。
