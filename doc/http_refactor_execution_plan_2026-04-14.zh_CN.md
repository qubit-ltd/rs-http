# rs-http 重构执行计划（2026-04-14）

## 目标

按用户指定顺序完成 11 项改进/特性。每项都必须满足：

1. 功能与测试完成。
2. `cargo test` 全通过。
3. 覆盖率（line coverage）>= 95%。
4. 提交一次 Git commit（提交消息英文）。
5. 在本文档勾选该项并记录 commit。

## 执行顺序与清单

### A. 优先改进（按要求顺序）

- [x] A1（对应“优先改进 #4”）抽取 `execute` / `execute_stream` 共享流程，减少重复逻辑。
  - 验收：不改变公开行为；相关测试与回归测试通过。
  - Commit: `cfe2131` (`refactor(client): extract shared send and status handling path`)

- [x] A2（对应“优先改进 #5”）收紧配置校验（重点：timeout/proxy 等边界）。
  - 验收：非法配置可在构建前被明确拒绝，并含清晰路径。
  - Commit: `37bacfd` (`refactor(validation): tighten timeout and proxy boundary checks`)

- [x] A3（对应“优先改进 #6”）将错误响应体预览限制与日志 body 限制解耦。
  - 验收：错误预览不再受 `logging.body_size_limit` 直接耦合影响。
  - Commit: `2c04389` (`refactor(error): decouple error preview limit from logging body limit`)

- [x] A4（对应“优先改进 #3”）将 timeout 分类改为阶段驱动，去除字符串启发式分类。
  - 验收：`connect/read/write/request` 分类稳定且测试可复现。
  - Commit: `efa309a` (`refactor(timeout): classify reqwest timeouts by execution phase`)

- [x] A5（对应“优先改进 #2”）增强 `Retry-After` 解析范围（支持 HTTP-date，并扩展适用状态码）。
  - 验收：`Retry-After` 秒数/日期格式解析正确，重试等待逻辑可验证。
  - Commit: `0a6cee3` (`refactor(retry): support HTTP-date Retry-After on retryable statuses`)

- [x] A6（对应“优先改进 #1”）移除阻塞等待（`std::thread::sleep`），替换为异步友好机制。
  - 验收：重试等待不阻塞 Tokio worker，行为与策略一致。
  - Commit: `bfd1138` (`refactor(retry): replace blocking retry-after wait with async sleep`)

### B. 可增加的实用特性（按要求顺序）

- [x] B1（对应“实用特性 #1”）扩展 `HttpClientOptions`，覆盖更多常用 reqwest 选项。
  - 验收：新增选项可从代码与配置加载并生效。
  - Commit: `feat(options): add common reqwest client settings`

- [ ] B2（对应“实用特性 #3”）新增 request/response 拦截器能力。
  - 验收：支持前后置拦截，顺序可控，错误可短路。
  - Commit: _pending_

- [ ] B3（对应“实用特性 #4”）支持流式上传请求体。
  - 验收：可发送流式 body，保留现有 body API 兼容性。
  - Commit: _pending_

- [ ] B4（对应“实用特性 #2”）增加细粒度重试策略（按状态码/错误类型等）。
  - 验收：策略可配置，行为有明确测试覆盖。
  - Commit: _pending_

- [ ] B5（对应“实用特性 #5”）SSE 自动重连与 `Last-Event-ID` 支持。
  - 验收：断线重连策略可控，事件 ID 透传并可恢复。
  - Commit: _pending_

## 每项执行模板

1. 实现改动（最小必要范围）。
2. 补充/更新测试（含边界与回归）。
3. 执行：
   - `cargo test`
   - `./coverage.sh text`（确认 line coverage >= 95%）
4. 自检变更范围（`git status` / `git --no-pager diff`）。
5. 提交英文 commit。
6. 回写本文档勾选项与 commit 哈希。
