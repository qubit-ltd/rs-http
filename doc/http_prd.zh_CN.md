# `rust-http` 产品需求文档（PRD）

## 文档信息

- 文档名称：`rust-http` 产品需求文档（PRD）
- 文档版本：`v1.1`
- 创建日期：`2026-04-08`
- 状态：`Draft`
- 对齐设计文档：`rust-common/rust-http/doc/http_design.zh_CN.md`
- 需求来源：`llmsdk/llmsdk-rust/rust-llmsdk-core/doc/java-porting-checklist.zh_CN.md`（4.1）
- 架构决策：SSE 作为 `rust-http::sse` 子模块内置实现

## 1. 背景

`llmsdk-rust` 正在从 Java 迁移。迁移清单明确 `qubit-http` 为优先建设的公共基础设施。  
若各 provider 直接裸用 `reqwest`，会导致超时、代理、日志脱敏、错误分类、SSE 解析行为不一致，后续统一成本高。

因此需要一个可复用的 `rust-http` 能力层，统一网络与流式语义并向上提供稳定接口。

## 2. 目标与非目标

### 2.1 产品目标

1. 提供统一 HTTP 客户端配置与构建能力，覆盖超时、代理、日志、敏感头、IPv4-only。
2. 提供统一请求执行入口（普通请求 + 流式请求）。
3. 提供内置 SSE 解码能力（`data:`、空行、`[DONE]`、JSON chunk 容错）。
4. 提供统一错误模型与重试提示语义，支持上层接入 `qubit-retry`。
5. 保障 provider 在无需重复造轮子的前提下快速接入。

### 2.2 非目标

1. 不替代 `reqwest` 全量 API。
2. 不处理非 HTTP 协议流解析（如 WebSocket/gRPC）。
3. 不在本模块绑定具体重试策略（仅提供分类与衔接点）。

## 3. 用户与使用场景

### 3.1 目标用户

1. `rust-llmsdk-*` provider 开发者。
2. `rust-llmsdk-core` 维护者。
3. 测试与平台工程师（需要统一观测与错误语义）。

### 3.2 核心场景

1. provider 发起普通 JSON API 请求。
2. provider 使用 `execute_stream` + `rust-http::sse` 获取强类型流式 chunk。
3. 统一开启/关闭请求响应日志并对敏感头自动脱敏。
4. 基于统一错误类型和 retry hint 做重试与故障排查。

## 4. 范围（Release Scope）

### 4.1 MVP（必须）

1. `HttpClientOptions`
2. `HttpClientFactory` + 默认实现
3. 默认 header 注入
4. 代理配置（含认证）
5. connect/read/write timeout
6. 日志开关与敏感头脱敏
7. 统一错误映射
8. 流式响应入口（字节流）
9. SSE 解码（事件/`[DONE]`/JSON chunk 容错）
10. `qubit-retry` 衔接点（retry hint）

### 4.2 增强项（M2）

1. IPv4-only 的可插拔 resolver 实现。
2. 更细粒度可观测字段（阶段耗时、连接信息）标准化。

## 5. 需求列表（含验收标准）

### PRD-HTTP-001：统一客户端选项

- 需求描述：提供统一配置结构，至少覆盖 `base_url/default_headers/timeouts/proxy/logging/sensitive_headers/ipv4_only`。
- 优先级：`P0`
- 验收标准：
  1. 提供默认值并与 Java 语义对齐（10/120/120 秒、日志默认开启等）。
  2. 不设置字段时行为稳定、可预测。

### PRD-HTTP-002：客户端工厂与默认实现

- 需求描述：提供 `HttpClientFactory` 抽象和 `ReqwestHttpClientFactory` 默认实现。
- 优先级：`P0`
- 验收标准：
  1. 可通过工厂构建客户端并执行请求。
  2. 工厂错误统一转为 `HttpError`。

### PRD-HTTP-003：请求构建与执行

- 需求描述：提供最小高频 API：`request/execute/execute_stream`。
- 优先级：`P0`
- 验收标准：
  1. 支持 `path/query/header/json/body`。
  2. 支持 `base_url + relative path` 解析。
  3. 非法 URL 明确报错。

### PRD-HTTP-004：Header 注入链

- 需求描述：支持默认 headers、可插拔 header injector、请求级覆盖。
- 优先级：`P0`
- 验收标准：
  1. 注入顺序固定：默认 -> 注入器 -> 请求级覆盖。
  2. 能满足认证头注入场景（如 `Authorization`）。

### PRD-HTTP-005：代理能力

- 需求描述：支持 `http/https/socks5` 代理及用户名密码认证。
- 优先级：`P0`
- 验收标准：
  1. 当 `enabled=true` 且 host/port 缺失时返回 `ProxyConfig` 错误。
  2. 代理认证配置生效且不在日志中泄露明文。

### PRD-HTTP-006：超时能力

- 需求描述：支持 connect/read/write/request timeout 语义。
- 优先级：`P0`
- 验收标准：
  1. connect 超时可配置并可触发对应错误。
  2. read/write 超时通过流程包装实现并可分类。
  3. request timeout 可作为兜底。

### PRD-HTTP-007：日志与敏感信息保护

- 需求描述：提供日志总开关和请求/响应 header/body 分级开关，敏感头自动脱敏。
- 优先级：`P0`
- 验收标准：
  1. 五个日志开关全部生效。
  2. 默认敏感头集合内字段必须脱敏。
  3. 脱敏策略：`<=4` 返回 `****`，否则保留前2后2。
  4. 二进制/未知编码 body 不打印原文。

### PRD-HTTP-008：统一错误模型

- 需求描述：统一错误类型，覆盖 URL、构建、代理、超时、传输、状态码、解码、SSE 协议/解析、取消。
- 优先级：`P0`
- 验收标准：
  1. 任一失败路径都落入 `HttpError`。
  2. 错误中包含必要上下文：method/url/status/message/source。

### PRD-HTTP-009：流式入口与 SSE 解码

- 需求描述：提供稳定字节流入口，并在 `rust-http::sse` 中完成事件与 JSON chunk 解码。
- 优先级：`P0`
- 验收标准：
  1. `execute_stream` 可稳定返回 chunk 流。
  2. 支持 `data:`、空行分帧、`[DONE]`。
  3. 默认宽松模式下坏 JSON chunk 跳过，不中断整条流。
  4. 严格模式可在坏 chunk 上返回错误。

### PRD-HTTP-010：重试衔接点

- 需求描述：提供 `RetryHint`（可重试/不可重试）判定接口。
- 优先级：`P0`
- 验收标准：
  1. `timeout/transport/429/5xx` 默认可重试。
  2. `SseProtocol/SseDecode/InvalidUrl/ProxyConfig` 默认不可重试。
  3. 上层可直接用于 `qubit-retry` 策略配置。

### PRD-HTTP-011：IPv4-only（增强）

- 需求描述：支持仅使用 IPv4 的解析/连接策略。
- 优先级：`P1`
- 验收标准：
  1. M1 可配置并验证参数合法。
  2. M2 完成 resolver 插件化实现后可端到端验证。

## 6. 非功能需求

1. 一致性：同类错误在不同 provider 中必须呈现相同错误种类。
2. 安全性：默认不泄露敏感头和敏感 body。
3. 可维护性：抽象层不转发 `reqwest` 全量 API，保持小而稳。
4. 可测试性：支持单测和集成测覆盖 HTTP + SSE 核心路径。

## 7. 依赖与约束

### 7.1 技术依赖

1. `reqwest`
2. `http`
3. `url`
4. `bytes`
5. `tokio`（超时包装）
6. `futures` / `tokio-stream`（流式处理）
7. `serde` / `serde_json`（SSE JSON chunk 解码）
8. `qubit-retry`（衔接，不强绑定）

### 7.2 上下游约束

1. 上游 provider 依赖统一错误模型、header 注入、SSE 解码能力。
2. `rust-http` 对上游暴露稳定语义，不暴露 `reqwest` 细节。

## 8. 里程碑与交付

### M1：最小可用 HTTP + SSE 能力（P0）

- 交付项：
  1. `options/factory/client/request/response/error/stream` 基础模块。
  2. `sse` 子模块（事件解码、done marker、JSON chunk 解码）。
  3. 日志开关、敏感头脱敏、代理、timeout、retry hint。
- 里程碑完成定义：
  1. 普通 JSON 请求可跑通。
  2. SSE 流式请求可产出强类型 chunk。
  3. 错误分类和日志安全符合 PRD。

### M2：增强与对齐（P1）

- 交付项：
  1. IPv4-only resolver 能力。
  2. 可观测信息增强与回归测试补全。
- 里程碑完成定义：
  1. IPv4-only 场景端到端可验证。
  2. 核心路径测试矩阵稳定。

## 9. 验收方案

### 9.1 单元测试验收

1. 默认值与配置覆盖。
2. 敏感头脱敏规则。
3. 错误分类映射与 retry hint 判定。
4. SSE 行切分、分帧、`[DONE]`、坏 chunk 容错/严格模式。

### 9.2 集成测试验收

1. 2xx/4xx/5xx 响应路径。
2. connect/read/write 超时路径。
3. 代理路径（含认证）。
4. 流式响应、SSE 解码与中断路径。

### 9.3 回归验收

1. provider 接入后无需重复实现日志/错误分类/SSE 解析逻辑。
2. 与 `http_design.zh_CN.md` 对齐项全部可追踪。

## 10. 风险与应对

1. 风险：`reqwest` 对 read/write timeout 无原生等价语义。
   - 应对：阶段化包装 + 明确错误分类 + 用例覆盖。
2. 风险：不同 provider 的 SSE 变体不一致。
   - 应对：提供 done marker 策略与严格/宽松两种解码模式。
3. 风险：日志能力过重导致性能损耗。
   - 应对：按开关启用，body 输出限制级别与长度。
4. 风险：过度封装导致 API 膨胀。
   - 应对：坚持最小高频 API，避免全量转发。

## 11. PRD 与设计文档对齐矩阵

| PRD 需求ID | 对齐设计章节 | 对齐说明 |
| --- | --- | --- |
| PRD-HTTP-001 | 5.1 | `HttpClientOptions` 结构与默认值 |
| PRD-HTTP-002 | 5.2 | `HttpClientFactory` 与默认实现 |
| PRD-HTTP-003 | 5.3 | `request/execute/execute_stream` |
| PRD-HTTP-004 | 5.4 | Header 注入机制与顺序 |
| PRD-HTTP-005 | 5.1, 8.1 | 代理字段与 `reqwest` 映射 |
| PRD-HTTP-006 | 5.1, 8.2 | connect/read/write/request timeout |
| PRD-HTTP-007 | 6.1, 6.2, 6.3 | 日志开关与脱敏策略 |
| PRD-HTTP-008 | 7.1 | 统一错误模型（含 SSE 错误） |
| PRD-HTTP-009 | 5.3, 5.5, 8.3, 9 | 流式入口与 SSE 子模块 |
| PRD-HTTP-010 | 7.2 | `RetryHint` 与 `qubit-retry` 衔接 |
| PRD-HTTP-011 | 8.4 | IPv4-only 分阶段实现 |

## 12. 发布判定（Go/No-Go）

满足以下条件可 `Go`：

1. P0 需求全部满足并通过验收。
2. 至少一个 provider 试点接入成功。
3. 无敏感信息泄露风险。
4. 关键错误路径与 SSE 路径可观测、可诊断。

不满足则 `No-Go`，继续修正后复验。
