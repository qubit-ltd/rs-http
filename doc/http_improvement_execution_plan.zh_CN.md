# `rs-http` 改进与特性落地计划（执行版）

## 1. 目标与约束

- 按既定优先级完成 9 项改造（5 项优先改进 + 4 项实用特性）。
- **每完成一项都必须：**
  - 补齐/新增完整单元测试与必要集成测试；
  - 运行 `cargo test` 并全部通过；
  - 运行覆盖率检查并保证 **line coverage >= 95%**；
  - 提交一次 Git commit（提交消息使用英文）；
  - 在本清单中勾选该项并记录对应 commit。
- 修改过程中保持向后兼容，除非该项明确要求变更行为。

## 2. 执行顺序（严格按序）

### 阶段 A：优先改进

- [x] A1. `ipv4_only` 真正生效（不再仅告警）
  - 实现要点：
    - 引入可测试、可配置的 IPv4-only 解析/连接约束机制；
    - 明确 HTTPS/SNI 与 Host header 场景下的行为；
    - 删除“仅告警未生效”的语义。
  - 测试要点：
    - `ipv4_only=true` 时只使用 IPv4；
    - `ipv4_only=false` 保持原行为；
    - 与 `base_url`、绝对 URL、代理场景的兼容性。
  - 验收命令：
    - `cargo test`
    - `./coverage.sh text`
  - Commit: `feat(ipv4): enforce ipv4-only resolver and reject ipv6 literals`

- [x] A2. 引入精确 `RequestTimeout` 错误分类
  - 实现要点：
    - 将总请求超时与 connect/read/write 超时区分；
    - 保持 `retry_hint()` 语义正确。
  - 测试要点：
    - client/request 级 request timeout 分类正确；
    - connect/read/write 分类不回归；
    - retryable 判定覆盖。
  - 验收命令：
    - `cargo test`
    - `./coverage.sh text`
  - Commit: `feat(timeout): classify request deadline timeout explicitly`

- [x] A3. 非 2xx 错误携带响应体摘要
  - 实现要点：
    - 在 `HttpError` 中加入可控长度的错误响应体预览；
    - 避免泄露敏感信息，限制长度并标记截断。
  - 测试要点：
    - 4xx/5xx 错误包含 preview；
    - 空 body / 二进制 body 处理正确；
    - 长 body 截断行为稳定。
  - 验收命令：
    - `cargo test`
    - `./coverage.sh text`
  - Commit: `feat(error): attach bounded response body preview for non-2xx`

- [x] A4. `create_with_options` 统一执行 `validate()`
  - 实现要点：
    - 保证 from_config 与 direct options 路径行为一致；
    - 错误类型与路径语义保持可理解。
  - 测试要点：
    - 直接调用 `create_with_options` 时校验失败可被阻止；
    - 合法配置不受影响。
  - 验收命令：
    - `cargo test`
    - `./coverage.sh text`
  - Commit: `refactor(factory): validate options at entry and keep proxy mapping`

- [x] A5. SSE 行/帧大小上限保护
  - 实现要点：
    - 增加 `max_line_bytes`、`max_frame_bytes`（含默认值）；
    - 超限时返回明确 `HttpError::sse_protocol`。
  - 测试要点：
    - 正常 SSE 不受影响；
    - 超长行/帧触发稳定错误；
    - chunk 边界场景覆盖。
  - 验收命令：
    - `cargo test`
    - `./coverage.sh text`
  - Commit: `feat(sse): enforce line and frame size limits for decoding`

### 阶段 B：实用特性

- [x] B1. 请求级重试覆盖策略（override）
  - 实现要点：
    - 在 `HttpRequest`/builder 增加请求级 retry policy；
    - 支持禁用、强制启用、方法策略覆盖、可选 `Retry-After` 支持。
  - 测试要点：
    - 覆盖默认策略、force enable/disable；
    - 429 + `Retry-After` 解析与等待行为。
  - 验收命令：
    - `cargo test`
    - `./coverage.sh text`
  - Commit: `feat(retry): add request-level retry override with optional Retry-After honoring`

- [x] B2. 取消能力（`CancellationToken`）
  - 实现要点：
    - 请求执行前和流式读取中支持取消；
    - 返回 `HttpErrorKind::Cancelled`。
  - 测试要点：
    - execute/execute_stream 均可取消；
    - 取消后资源可释放，不影响后续请求。
  - 验收命令：
    - `cargo test`
    - `./coverage.sh text`
  - Commit: `feat(cancel): add CancellationToken support for execute and streaming paths`

- [x] B3. 异步 Header Injector
  - 实现要点：
    - 在保持现有同步注入器兼容的前提下，新增 async 注入器链；
    - 注入顺序和覆盖优先级保持确定性。
  - 测试要点：
    - 异步注入成功路径；
    - 异步注入失败短路；
    - 同步 + 异步混用顺序验证。
  - 验收命令：
    - `cargo test`
    - `./coverage.sh text`
  - Commit: `feat(headers): add async header injector chain with deterministic ordering`

- [x] B4. 表单 / multipart / NDJSON 请求体支持
  - 实现要点：
    - builder 增加 `form_body`、`multipart_body`、`ndjson_body`；
    - 自动 `Content-Type` 策略与现有 text/json 语义一致。
  - 测试要点：
    - 三类请求体序列化正确；
    - content-type 自动填充与覆盖行为正确；
    - 与 query/header/timeout 组合不回归。
  - 验收命令：
    - `cargo test`
    - `./coverage.sh text`
  - Commit: `feat(body): support form multipart and ndjson request payload builders`

## 3. 每项执行模板

每项按以下步骤循环执行：

1. 实现该项代码改动（最小改动面）。
2. 补齐测试（正常路径、错误路径、边界场景）。
3. 运行：
   - `cargo test`
   - `./coverage.sh text`
4. 若覆盖率 < 95%，继续补测试直至达标。
5. `git status` / `git --no-pager diff` 自检。
6. `git add` + `git commit`（英文消息）。
7. 在本文档勾选该项并写入 commit hash。

## 4. 进度记录

- 当前状态：`In Progress`
- 最后更新时间：`2026-04-14`
