# `rust-http` 需求对齐评估与测试方案

## 文档信息

- 文档名称：`rust-http` 需求对齐评估与测试方案
- 文档版本：`v1.0`
- 创建日期：`2026-04-10`
- 评估对象：`rust-common/rust-http`
- 评估基线：
  - 需求文档：`doc/http_prd.zh_CN.md`
  - 设计文档：`doc/http_design.zh_CN.md`
  - 当前实现：`src/`
  - 当前测试：`tests/`

## 1. 评估方法与结论摘要

本次评估基于以下事实：

1. 对 `PRD / 设计文档 / 源码 / 现有测试` 做逐项对照。
2. 本地执行 `cargo test`，结果为 `88 passed, 0 failed`。
3. 对关键路径额外核查了 `reqwest` 依赖能力边界。

结论：

1. 当前实现已经覆盖 PRD 的主干能力，尤其是 `options / factory / request / execute / execute_stream / SSE / retry hint` 已具备可用形态。
2. 但当前状态不能判定为“与 PRD 完全对齐”。
3. 最主要的未闭环项有两个：
   - `PRD-HTTP-005` 中声明的 `socks5` 代理支持未形成可验证的运行时能力。
   - `PRD-HTTP-008` 中声明的 `Cancelled` 错误类型只有定义，没有实际触发路径。
4. 另外，`timeout / logging / SSE 协议异常 / 代理链路` 的测试覆盖仍有明显空白，现有 88 个测试更多证明“核心 happy path 可用”，还不足以证明“验收标准已全部闭环”。

## 2. PRD 对齐评估

### 2.1 总体判断

| PRD 编号 | 结论 | 说明 |
| --- | --- | --- |
| `PRD-HTTP-001` | 对齐 | `HttpClientOptions`、默认值、配置加载与校验已落地。 |
| `PRD-HTTP-002` | 对齐 | `HttpClientFactory` 与 `create_from_config()` 已落地。 |
| `PRD-HTTP-003` | 对齐 | `request / execute / execute_stream` 已具备，URL 解析与非法 URL 错误已实现。 |
| `PRD-HTTP-004` | 对齐 | 默认头、注入器、请求级覆盖顺序已实现并已有集成测试。 |
| `PRD-HTTP-005` | 部分对齐 | `http / https` 代理路径具备；`socks5` 只停留在配置和枚举层，未闭环。 |
| `PRD-HTTP-006` | 部分对齐 | `read / write / request timeout` 机制存在，但 `connect timeout`、`request timeout` 缺少行为级验证；`write timeout` 语义边界偏宽。 |
| `PRD-HTTP-007` | 部分对齐 | 开关、脱敏、body 截断都已实现，但缺少日志输出的端到端断言。 |
| `PRD-HTTP-008` | 部分对齐 | 统一错误模型已建立，但 `Cancelled` 没有运行时触发路径。 |
| `PRD-HTTP-009` | 基本对齐 | 流式入口与 SSE 解码已实现，现有测试覆盖核心路径；协议异常和端到端 SSE 仍需补强。 |
| `PRD-HTTP-010` | 对齐 | `RetryHint` 与默认分类规则已落地。 |
| `PRD-HTTP-011` | 按 M1 对齐 | `ipv4_only` 当前仅是可配置选项，传输层强制能力尚未实现，符合 PRD 中的增强项描述。 |

### 2.2 关键证据

#### `PRD-HTTP-001` 到 `PRD-HTTP-004`

1. `HttpClientOptions` 已覆盖 `base_url / default_headers / timeouts / proxy / logging / sensitive_headers / ipv4_only`。
2. `HttpClientFactory` 已将配置映射到 `reqwest::ClientBuilder`。
3. `HttpClient` 已提供 `request / execute / execute_stream`。
4. header 合并顺序为“默认头 -> 注入器 -> 请求级覆盖”。

对应实现：

- `src/options/http_client_options.rs`
- `src/http_client_factory.rs`
- `src/http_client.rs`
- `src/request/http_request_builder.rs`

现有测试证明：

- `tests/http_client/http_client_tests.rs`
- `tests/request/http_request_builder_tests.rs`
- `tests/factory/reqwest_http_client_factory_tests.rs`

#### `PRD-HTTP-005` 代理能力存在未闭环

当前实现确实支持：

1. `ProxyType::{Http,Https,Socks5}` 枚举与配置解析。
2. 工厂中按 `proxy_type.scheme()` 组装代理 URL。
3. 代理用户名密码通过 `proxy.basic_auth(...)` 注入。

但当前实现不能证明 `socks5` 已真正可用，原因如下：

1. `Cargo.toml` 中 `reqwest` 仅启用了 `json / stream / query`，未启用 `socks` feature。
2. `reqwest` 源码明确写明：`SOCKS4 / SOCKS5 / SOCKS5H` 只有在 `socks` feature 启用时才支持。
3. 现有测试只校验了配置合法性和工厂对象可创建，没有任何 `socks5` 端到端代理测试。

因此，对 `PRD-HTTP-005` 的判断是：

1. `http / https` 代理：实现存在，但也缺端到端转发和认证验证。
2. `socks5` 代理：当前不能算已验收。

#### `PRD-HTTP-006` timeout 语义基本具备，但还没完全验收

当前实现：

1. `connect_timeout` 直接设置到 `reqwest::ClientBuilder`。
2. `write_timeout` 通过 `tokio::time::timeout(..., builder.send())` 包装。
3. `read_timeout` 分别包装 `response.bytes()` 和 `response.bytes_stream().next()`。
4. `request_timeout` 支持 client 级和 request 级覆盖。

这里有两个需要在测试方案中写清楚的点：

1. 当前 `write_timeout` 包住的是 `builder.send()` 整体，不只是“写请求体”，还包含等待响应头返回的时间。
2. `connect_timeout` 与 `request_timeout` 虽然有代码入口，但现有测试没有证明行为和错误分类。

因此，`PRD-HTTP-006` 当前可判定为“机制已实现，验收未闭环”。

#### `PRD-HTTP-007` 日志与敏感信息保护实现存在，但验证不足

当前实现已经具备：

1. 五个日志开关。
2. 敏感头脱敏规则：`<=4 => ****`，否则保留前 2 后 2。
3. `TRACE` 级才打印。
4. body 长度截断。
5. 非 UTF-8 body 以 `<binary N bytes>` 形式输出。

但当前仍缺少：

1. 对五个日志开关“逐一生效”的端到端测试。
2. 对二进制 body、超长 body、streaming 响应日志行为的断言。
3. 对代理认证信息“不泄露到日志”的行为测试。

因此，当前更接近“实现已落地，但验收证据不足”。

#### `PRD-HTTP-008` 错误模型广度够，但 `Cancelled` 未接线

当前 `HttpErrorKind` 已定义：

1. `InvalidUrl`
2. `BuildClient`
3. `ProxyConfig`
4. `ConnectTimeout`
5. `ReadTimeout`
6. `WriteTimeout`
7. `Transport`
8. `Status`
9. `Decode`
10. `SseProtocol`
11. `SseDecode`
12. `Cancelled`
13. `Other`

但代码检视显示：

1. `Cancelled` 仅存在于类型定义和构造函数。
2. 当前请求执行、流式读取、SSE 解码都没有把任何运行时情况映射为 `Cancelled`。

这意味着 PRD 里“取消”这一类错误只在类型层声明，没有行为落点，属于真实缺口。

#### `PRD-HTTP-009` 流式与 SSE 主路径可用

当前实现已经具备：

1. `execute_stream()` 返回字节流。
2. `line_decoder` 负责按 `\n` / `\r\n` 切分。
3. `frame_decoder` 负责按空行分帧。
4. `decode_json_chunks()` 支持宽松模式跳过坏 JSON。
5. `decode_json_chunks_with_mode(..., Strict)` 支持严格模式报错。
6. `DoneMarkerPolicy` 支持 `[DONE]` 和自定义完成标记。

当前测试已经覆盖：

1. 多行 `data:` 合并。
2. comment 行忽略。
3. 宽松模式坏 chunk 跳过。
4. 严格模式坏 chunk 失败。
5. 自定义 done marker。

但还缺少：

1. 非 UTF-8 SSE 行触发 `SseProtocol`。
2. chunk 边界打散后的逐步解码。
3. `execute_stream()` + `decode_events()` 的端到端联测。

#### `PRD-HTTP-010` RetryHint 已对齐

当前分类规则与 PRD 一致：

1. `timeout / transport / 429 / 5xx` 为可重试。
2. `InvalidUrl / ProxyConfig / SseProtocol / SseDecode` 为不可重试。

该项实现与现有测试是一致的。

#### `PRD-HTTP-011` IPv4-only 仅完成 M1，不是 M2

当前状态：

1. `ipv4_only` 字段已存在，可从配置加载。
2. 工厂在启用时只打印 warning，不真正修改 resolver。

这与 PRD 中“增强项 / M1 仅配置、M2 再做传输层 resolver”的描述是一致的，因此不构成 P0 偏差。

### 2.3 差距清单

建议把下面几项视为当前版本最重要的对齐缺口：

1. `P0` 缺口：`socks5` 代理未形成真实可用实现。
2. `P0` 缺口：`Cancelled` 错误没有任何运行时映射路径。
3. `P0` 验收缺口：`connect_timeout`、`request_timeout` 缺行为测试。
4. `P0` 验收缺口：日志五开关、二进制 body、代理认证脱敏缺端到端测试。
5. `P0` 验收缺口：SSE 协议异常和端到端流式链路缺测试。

## 3. 测试策略

### 3.1 测试目标

本方案目标不是“把现有测试继续堆厚”，而是围绕 PRD 验收标准补足以下三类证据：

1. 功能正确性：PRD 声明的每个能力都有对应行为测试。
2. 语义稳定性：错误分类、日志安全、超时边界在不同路径下语义一致。
3. 回归可追踪性：每个 PRD 条目都能映射到测试文件和用例编号。

### 3.2 测试分层

1. 单元测试：验证配置解析、脱敏、纯解码器、错误分类。
2. 组件测试：验证 `HttpClientFactory`、`HttpClient`、`HeaderInjector`、timeout 包装。
3. 集成测试：验证真实 HTTP server / proxy / stream / SSE 行为。
4. 受限场景测试：`socks5`、IPv4-only 等需要额外 harness 或 feature gating 的场景。

### 3.3 建议新增测试基础设施

建议新增以下测试辅助模块：

1. `tests/common/proxy_server.rs`
   - 提供最小 HTTP forward proxy。
   - 提供最小 HTTPS `CONNECT` proxy。
   - 记录代理收到的认证头、目标地址和转发结果。
2. `tests/common/tracing_capture.rs`
   - 捕获 `tracing::trace!` 输出，供日志行为断言。
3. 扩展 `tests/common/one_shot_server.rs`
   - 增加“延迟响应头”“延迟首包”“SSE chunk 跨边界切分”“返回二进制 body”等场景。
4. `tests/common/socks_proxy.rs`
   - 在启用 `reqwest` 的 `socks` feature 后再落地。
   - 当前可先标记为 `Pending`。

### 3.4 测试执行建议

1. 默认 CI 跑：
   - 单元测试
   - 普通 HTTP 集成测试
   - SSE 集成测试
   - 日志断言测试
2. 条件化 CI 跑：
   - `socks5` 代理测试
   - IPv4-only resolver 端到端测试
3. 若引入 feature gating：
   - `cargo test`
   - `cargo test --features socks-proxy-tests`

## 4. 详细测试矩阵

下表按 PRD 编号组织，`当前状态` 分为：

- `已有`：仓库内已有明确测试覆盖。
- `需补`：实现存在但缺测试。
- `阻塞`：当前实现本身未闭环，测试需在实现补齐后落地。

### 4.1 `PRD-HTTP-001` 统一客户端选项

| 用例 ID | 当前状态 | 级别 | 场景 | 关键断言 | 建议文件 |
| --- | --- | --- | --- | --- | --- |
| `OPT-001` | 已有 | 单元 | 默认配置 | 默认值与 PRD 文档一致 | `tests/options/http_client_options_tests.rs` |
| `OPT-002` | 已有 | 单元 | `base_url` / `ipv4_only` / `sensitive_headers` 加载 | 能从 root/prefix 正确解析 | `tests/options/http_client_options_tests.rs` |
| `OPT-003` | 已有 | 单元 | `default_headers` 子键与 JSON 双格式 | 两种输入都可解析为 `HeaderMap` | `tests/options/http_client_options_tests.rs` |
| `OPT-004` | 需补 | 单元 | 超时值为 `0` 或边界值 | 明确是否允许，若允许要文档化 | `tests/options/timeout_options_tests.rs` |
| `OPT-005` | 需补 | 组件 | `HttpClientOptions::validate()` 全量组合 | 代理错误与日志错误只返回第一类明确异常 | `tests/options/http_client_options_tests.rs` |

### 4.2 `PRD-HTTP-002` 客户端工厂与默认实现

| 用例 ID | 当前状态 | 级别 | 场景 | 关键断言 | 建议文件 |
| --- | --- | --- | --- | --- | --- |
| `FAC-001` | 已有 | 组件 | 默认 options 创建 client | 成功返回 `HttpClient` | `tests/factory/reqwest_http_client_factory_tests.rs` |
| `FAC-002` | 已有 | 组件 | `create_from_config()` | prefix path 保留，配置校验前置 | `tests/factory/reqwest_http_client_factory_tests.rs` |
| `FAC-003` | 需补 | 组件 | client build 失败映射 | 失败应转为 `HttpError::BuildClient` 或配置错误 | `tests/factory/reqwest_http_client_factory_tests.rs` |
| `FAC-004` | 需补 | 组件 | `proxy.enabled=false` | 不继承环境代理 | `tests/factory/reqwest_http_client_factory_tests.rs` |

### 4.3 `PRD-HTTP-003` 请求构建与执行

| 用例 ID | 当前状态 | 级别 | 场景 | 关键断言 | 建议文件 |
| --- | --- | --- | --- | --- | --- |
| `REQ-001` | 已有 | 单元 | `query/header/json/body/timeout` 构建 | builder 产物字段正确 | `tests/request/http_request_builder_tests.rs` |
| `REQ-002` | 已有 | 集成 | `base_url + relative path` | 请求路径正确拼接 | `tests/http_client/http_client_tests.rs` |
| `REQ-003` | 已有 | 集成 | 无 `base_url` 的相对路径 | 返回 `InvalidUrl` | `tests/http_client/http_client_tests.rs` |
| `REQ-004` | 需补 | 集成 | 绝对 URL 优先于 `base_url` | 绝对 URL 不被再次 join | `tests/http_client/http_client_tests.rs` |
| `REQ-005` | 需补 | 集成 | 非法绝对 URL / 非法 header 值 | 返回明确错误且不发起网络请求 | `tests/http_client/http_client_tests.rs` |

### 4.4 `PRD-HTTP-004` Header 注入链

| 用例 ID | 当前状态 | 级别 | 场景 | 关键断言 | 建议文件 |
| --- | --- | --- | --- | --- | --- |
| `HDR-001` | 已有 | 集成 | 默认头 -> 注入器 -> 请求级覆盖 | 最终值按优先级覆盖 | `tests/http_client/http_client_tests.rs` |
| `HDR-002` | 需补 | 组件 | 多个注入器顺序 | 注入器注册顺序稳定 | `tests/http_client/http_client_tests.rs` |
| `HDR-003` | 需补 | 组件 | 注入器返回错误 | 返回 `HttpError`，且请求不落网 | `tests/http_client/http_client_tests.rs` |
| `HDR-004` | 需补 | 组件 | `clear_header_injectors()` | 清理后不再注入任何 header | `tests/http_client/http_client_tests.rs` |

### 4.5 `PRD-HTTP-005` 代理能力

| 用例 ID | 当前状态 | 级别 | 场景 | 关键断言 | 建议文件 |
| --- | --- | --- | --- | --- | --- |
| `PROXY-001` | 已有 | 单元 | 缺 host / port | 返回 `ProxyConfig` | `tests/factory/reqwest_http_client_factory_tests.rs` |
| `PROXY-002` | 已有 | 单元 | password 无 username | 返回 `ProxyConfig` | `tests/factory/reqwest_http_client_factory_tests.rs` |
| `PROXY-003` | 需补 | 集成 | HTTP proxy 转发 | 代理收到请求且上游返回正确 | `tests/proxy/http_proxy_tests.rs` |
| `PROXY-004` | 需补 | 集成 | HTTPS `CONNECT` proxy | 建立隧道并可访问 HTTPS 目标 | `tests/proxy/https_proxy_tests.rs` |
| `PROXY-005` | 需补 | 集成 | 代理 basic auth | 代理收到 `Proxy-Authorization`，业务请求头不混淆 | `tests/proxy/http_proxy_tests.rs` |
| `PROXY-006` | 需补 | 日志集成 | 代理认证不泄露日志 | 日志中不出现 proxy 用户名/密码明文 | `tests/logging/logging_policy_tests.rs` |
| `PROXY-007` | 阻塞 | 集成 | SOCKS5 代理转发 | 只有启用 `reqwest` `socks` feature 后才能通过 | `tests/proxy/socks5_proxy_tests.rs` |

### 4.6 `PRD-HTTP-006` 超时能力

| 用例 ID | 当前状态 | 级别 | 场景 | 关键断言 | 建议文件 |
| --- | --- | --- | --- | --- | --- |
| `TIME-001` | 需补 | 集成 | `connect_timeout` | 触发 `ConnectTimeout`，并带 `method/url` | `tests/http_client/http_client_timeout_tests.rs` |
| `TIME-002` | 已有 | 集成 | `write_timeout` | 当前实现可触发 `WriteTimeout` | `tests/http_client/http_client_tests.rs` |
| `TIME-003` | 已有 | 集成 | buffered body `read_timeout` | 返回 `ReadTimeout` | `tests/http_client/http_client_tests.rs` |
| `TIME-004` | 已有 | 集成 | streaming `read_timeout` | 第二个 chunk 超时后返回 `ReadTimeout` | `tests/http_client/http_client_tests.rs` |
| `TIME-005` | 需补 | 集成 | client-level `request_timeout` | 整体请求超时后返回 timeout 类错误 | `tests/http_client/http_client_timeout_tests.rs` |
| `TIME-006` | 需补 | 集成 | request-level `timeout()` 覆盖 client 默认值 | 请求级 timeout 生效并覆盖 client 默认值 | `tests/http_client/http_client_timeout_tests.rs` |
| `TIME-007` | 需补 | 组件 | timeout 错误 retry hint | `Connect/Read/Write` 全为 `Retryable` | `tests/retry_hint/retry_hint_tests.rs` |

补充说明：

1. `TIME-001` 需要特别注意稳定性，不建议依赖公网地址。
2. 更稳妥的做法是构造“可连通 TCP 不返回任何响应头”和“不可达地址”两类场景，分别验证 `write_timeout` 与 `connect_timeout`。
3. 若团队希望“write timeout 只覆盖 socket 写阶段”，则需要先调整实现，再落测试。

### 4.7 `PRD-HTTP-007` 日志与敏感信息保护

| 用例 ID | 当前状态 | 级别 | 场景 | 关键断言 | 建议文件 |
| --- | --- | --- | --- | --- | --- |
| `LOG-001` | 已有 | 单元 | 敏感头脱敏规则 | `<=4 => ****`，长值保留前 2 后 2 | `tests/logging/masker_tests.rs` |
| `LOG-002` | 需补 | 日志集成 | `enabled=false` | 不输出任何请求/响应 trace | `tests/logging/logging_policy_tests.rs` |
| `LOG-003` | 需补 | 日志集成 | 五个日志开关逐项生效 | 只输出允许的 header/body 维度 | `tests/logging/logging_policy_tests.rs` |
| `LOG-004` | 需补 | 日志集成 | 二进制 body | 日志输出 `<binary N bytes>`，不打印原文 | `tests/logging/logging_policy_tests.rs` |
| `LOG-005` | 需补 | 日志集成 | 超长 body | 按 `body_size_limit` 截断并追加标记 | `tests/logging/logging_policy_tests.rs` |
| `LOG-006` | 需补 | 日志集成 | stream 响应日志 | 只记录响应头，不提前消费 body stream | `tests/logging/logging_policy_tests.rs` |
| `LOG-007` | 需补 | 日志集成 | 敏感响应头掩码 | `Set-Cookie` 等响应头同样被脱敏 | `tests/logging/logging_policy_tests.rs` |

### 4.8 `PRD-HTTP-008` 统一错误模型

| 用例 ID | 当前状态 | 级别 | 场景 | 关键断言 | 建议文件 |
| --- | --- | --- | --- | --- | --- |
| `ERR-001` | 已有 | 集成 | 非 2xx 状态码 | 含 `kind/status/method/url/message` | `tests/http_client/http_client_tests.rs` |
| `ERR-002` | 需补 | 组件 | `BuildClient` 错误 | 包含 `source` | `tests/error/http_error_tests.rs` |
| `ERR-003` | 需补 | 集成 | 传输错误映射 | 返回 `Transport` 且包含 `source` | `tests/http_client/http_client_error_tests.rs` |
| `ERR-004` | 需补 | 集成 | 解码错误映射 | `HttpResponse::text/json` 错误包含 `status/url` | `tests/http_client/http_response_tests.rs` |
| `ERR-005` | 需补 | SSE 单元 | `SseProtocol` | 非 UTF-8 SSE 行触发 `SseProtocol` | `tests/sse/line_decoder_tests.rs` |
| `ERR-006` | 阻塞 | 组件/集成 | `Cancelled` | 需先定义取消语义，再验证映射 | `tests/http_client/http_client_cancel_tests.rs` |

### 4.9 `PRD-HTTP-009` 流式入口与 SSE 解码

| 用例 ID | 当前状态 | 级别 | 场景 | 关键断言 | 建议文件 |
| --- | --- | --- | --- | --- | --- |
| `SSE-001` | 已有 | 单元 | 多行 `data:` / `event` / `id` / `retry` | 能正确组帧 | `tests/sse/mod_tests.rs` |
| `SSE-002` | 已有 | 单元 | comment 行忽略 | `:` 行不会进事件数据 | `tests/sse/mod_tests.rs` |
| `SSE-003` | 已有 | 单元 | 宽松模式坏 JSON 跳过 | 不中断整条流 | `tests/sse/json_decoder_tests.rs` |
| `SSE-004` | 已有 | 单元 | 严格模式坏 JSON 报错 | 返回 `SseDecode` | `tests/sse/json_decoder_tests.rs` |
| `SSE-005` | 已有 | 单元 | 自定义 done marker | 正确产出 `SseChunk::Done` | `tests/sse/json_decoder_tests.rs` |
| `SSE-006` | 需补 | 单元 | 非 UTF-8 行 | 返回 `SseProtocol` | `tests/sse/line_decoder_tests.rs` |
| `SSE-007` | 需补 | 单元 | trailing buffer 无空行结尾 | 最后一个事件也能被 flush | `tests/sse/frame_decoder_tests.rs` |
| `SSE-008` | 需补 | 单元 | chunk 边界打散 | 跨 chunk 的一行/一帧仍能正确解析 | `tests/sse/line_decoder_tests.rs` |
| `SSE-009` | 需补 | 单元 | 上游 stream error 透传 | 下游立即收到原错误 | `tests/sse/line_decoder_tests.rs` |
| `SSE-010` | 需补 | 集成 | `execute_stream() + decode_events()` 联测 | 从真实 HTTP chunked SSE 响应解码成功 | `tests/sse/sse_integration_tests.rs` |
| `SSE-011` | 需补 | 集成 | 流中途断开 | 已产出的事件保留，后续返回错误 | `tests/sse/sse_integration_tests.rs` |

### 4.10 `PRD-HTTP-010` 重试衔接点

| 用例 ID | 当前状态 | 级别 | 场景 | 关键断言 | 建议文件 |
| --- | --- | --- | --- | --- | --- |
| `RETRY-001` | 已有 | 单元 | `timeout/transport` | 默认 `Retryable` | `tests/retry_hint/retry_hint_tests.rs` |
| `RETRY-002` | 已有 | 单元 | `429/5xx/4xx` | `429/5xx` 可重试，`4xx` 不可重试 | `tests/retry_hint/retry_hint_tests.rs` |
| `RETRY-003` | 已有 | 单元 | `InvalidUrl/ProxyConfig/SseProtocol/SseDecode` | 默认不可重试 | `tests/retry_hint/retry_hint_tests.rs` |
| `RETRY-004` | 需补 | 集成 | timeout/error 真值与 retry hint 一致 | 行为测试与静态分类保持一致 | `tests/http_client/http_client_timeout_tests.rs` |

### 4.11 `PRD-HTTP-011` IPv4-only

| 用例 ID | 当前状态 | 级别 | 场景 | 关键断言 | 建议文件 |
| --- | --- | --- | --- | --- | --- |
| `IPV4-001` | 已有 | 单元 | 选项解析 | `ipv4_only` 可读可写 | `tests/options/http_client_options_tests.rs` |
| `IPV4-002` | 需补 | 组件 | 工厂启用 `ipv4_only` | 当前仅保留配置并给出 warning | `tests/factory/reqwest_http_client_factory_tests.rs` |
| `IPV4-003` | 阻塞 | 集成 | 双栈目标下强制走 IPv4 | 需 resolver 能力落地后再验收 | `tests/network/ipv4_only_tests.rs` |

## 5. 建议的补测优先级

### 5.1 第一优先级：补齐 P0 闭环

1. `PROXY-003` 到 `PROXY-007`
2. `TIME-001`、`TIME-005`、`TIME-006`
3. `LOG-002` 到 `LOG-007`
4. `ERR-005`
5. `SSE-010`、`SSE-011`

### 5.2 第二优先级：消除语义歧义

1. 明确 `write_timeout` 的官方语义边界。
2. 明确 `request_timeout` 触发时的错误种类是否继续复用 `ConnectTimeout`。
3. 明确是否要支持运行时 `Cancelled`，以及取消来源是 `JoinHandle abort`、stream drop，还是显式 cancellation token。

### 5.3 第三优先级：增强项

1. `socks5` 真实代理联测。
2. `ipv4_only` 端到端联测。
3. 更细粒度的日志可观测字段回归。

## 6. 推荐落地顺序

建议按下面顺序实施：

1. 先补测试基础设施：
   - `proxy_server`
   - `tracing_capture`
   - `one_shot_server` 扩展
2. 再补 P0 验收测试：
   - proxy
   - timeout
   - logging
   - SSE e2e
3. 然后修实现缺口：
   - `reqwest` 开启 `socks` feature
   - 明确 `Cancelled` 语义并接线
4. 最后补增强项测试：
   - `ipv4_only`
   - 更复杂网络场景

## 7. 最终判断

截至 `2026-04-10`，`rust-http` 的当前实现可以认为：

1. 已经达到了“核心可用”的状态。
2. 已经与 PRD 主干方向基本一致。
3. 但尚未达到“所有 PRD 验收项均有实现和测试证据支持”的状态。

如果要给出一句收敛判断：

`rust-http` 当前是“核心能力基本对齐，P0 中仍有少量实现缺口和若干验收缺口”，其中最需要优先修正的是 `socks5` 代理闭环、`Cancelled` 错误接线，以及 timeout/logging/SSE 的行为级测试补齐。
