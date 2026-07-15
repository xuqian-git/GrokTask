# GrokTask 独立应用重构计划

状态：待用户审阅后交给 Grok 实施。

## 1. 工作方式与所有权

- Codex 负责本目录中的需求、架构、协议、UX、迁移与验收文档。
- 所有产品代码、测试代码、构建脚本、配置文件和 README 实现由 Grok 编写。
- Codex 在每个实现阶段后审查 diff、运行测试并形成具体 findings。
- Findings 原样交给 Grok 修复；Codex 再审查，直到没有阻断问题。
- `/Users/qian/project/AskHuman` 是只读架构与 UI 参考，不得修改。

当前工作分支：`codex/groktask-standalone`。

## 2. 输入文档

Grok 开始前必须完整阅读：

- `docs/specs/product.md`
- `docs/specs/architecture.md`
- `docs/specs/acp-runtime.md`
- `docs/specs/conversation-stream.md`
- `docs/specs/cli-mcp.md`
- `docs/specs/persistence-ipc.md`
- `docs/specs/integrations.md`
- `docs/acceptance.md`
- `docs/research/acp-conversation-flow.md`

如实现发现文档冲突，停止相关部分并列出冲突，不自行改变产品决策。

## 3. 目标仓库结构

```text
Cargo.toml                     # optional workspace wrapper
package.json
pnpm-lock.yaml
vite.config.ts
tsconfig.json
src/
src-tauri/
  Cargo.toml
  tauri.conf.json
  capabilities/
  icons/
  src/
tests/                         # frontend/integration fixtures as appropriate
docs/
.github/workflows/
README.md
LICENSE
```

最终不保留运行时 Node companion、plugin manifest、hooks、skills 或 localhost HTML server。

## 4. Phase 0：建立可构建骨架

交付：

- Tauri 2 + Vue 3 + TypeScript + Vite 最小应用。
- 单一 `GrokTask` Rust binary，多角色 CLI dispatch。
- production custom-protocol 资源嵌入。
- 基础 lint/typecheck/unit test/build scripts。
- macOS、Windows、Linux CI compile matrix。
- app identifier、窗口 label、图标占位与版本来源统一。

验收：

- `pnpm build` 通过。
- `cargo test --manifest-path src-tauri/Cargo.toml` 通过。
- `cargo build --release --features custom-protocol` 在当前平台通过并打开非白屏窗口。
- `GrokTask --help` 与 `GrokTask mcp` 角色不会错误初始化 GUI。

## 5. Phase 1：DTO、配置、SQLite 与 IPC

交付：

- config schema、原子读写、reload 与 validation。
- SQLite migrations、Task/Turn/recovery operation、start submission dedupe、owner 与 terminationCause、repositories、持久化 retention deadline、ui_state generation/revision。
- NDJSON codec、Unix socket、Windows named pipe abstraction、handshake/version/fingerprint。
- daemon single-instance、detached start、status/stop/restart 和 graceful replacement。
- hidden task supervisor、Unix process group/Windows Job Object、daemon-death control-pipe cleanup。
- GUI host single-instance导航 IPC 的基础实现。

测试：

- config 缺失/未知字段/非法文件/原子写。
- migration rollback、WAL、transaction-before-broadcast、retention 不删除活动任务。
- codec roundtrip、frame limit、invalid JSON、EOF、slow subscriber。
- snapshot/delta barrier：独立 SQLite producer、per-subscriber bounded backlog、response/end 前不发 live、持续 ACP mutation 不阻塞、单 item fragment、ui_state 同 barrier、selection/subscription epoch、generation reset 原子 resnapshot。
- Unix permission；Windows target compile 与 named-pipe ACL 单测/集成测试。
- 并发 daemon start 只有一个成功 bind。
- off/active/always + connection-scoped window/request lease/transfer、idle MCP、warm session TTL/LRU/stale repair 与持久化 continuation deadline、clear tombstone race、queued/recovering/request-aware replacement defer/handoff crash。

## 6. Phase 2：ACP client 与 reducer

交付：

- 安全 spawn 参数 builder 与 Grok discovery/doctor。
- initialize/auth/new/prompt/cancel/load 的 ACP v1 client。
- per-session actor、prompt serialization、late-event drain、process-tree termination。
- 严格时序 reducer、stream buffer、tool merge、plan snapshot、usage/model metadata、diagnostic redaction。
- replay staging 与 atomic reconciliation。
- fake ACP agent fixture，能生成乱序/迟到/重复/重放/崩溃场景。

测试必须先于真实 Grok E2E 覆盖：

- read/write argv 精确值。
- Thought → Tool → Thought → Reply 顺序。
- token chunk coalescing 与 Unicode。
- update-before-create、duplicate tool update、plan full replacement 与原子 plan_finalize。
- stopReason/terminationCause precedence 全映射、live user echo 去重、permission read/write/cancel handler 与语义 item lifecycle。
- prompt response 后迟到 chunk。
- load response 前 replay notification；并发 load 去重。
- cancel 软成功、TERM fallback、KILL fallback。
- stderr flood/oversized frame/unknown xAI extension 不死锁不 panic。
- terminal output-before-metadata/exit-before-metadata、ANSI 跨 chunk 与 raw/normalized 容量预算。

## 7. Phase 3：Daemon、CLI 与 MCP

交付：

- TaskManager、并发上限、per-turn client-owned/daemon-owned 语义、immutable RunResult 与 `(taskId,turnId)` conditional operations。
- CLI `run/start/status/wait/cancel/tasks/doctor/daemon`。
- MCP stdio server 的六个工具（`run` / `start` / `continue` / `status` / `wait` / `cancel`）与 structuredContent。
- task status/error/result DTO 单源复用。
- crash startup recovery 与 write-task interrupted 保护。

测试：

- MCP tool list/schema/description snapshot。
- mode/cwd/task validation。
- blocking run、submissionId exactly-once async start + repeated wait、Turn/recovery conditional cancel；queued/pre-prompt/running cancel 分支与 T1/T2 并发目标稳定。
- MCP disconnect 取消 run，但不取消 start。
- MCP `notifications/cancelled` 只取消 request 绑定 turn；取消 wait/status 不影响 daemon-owned turn。
- CLI JSON 与 MCP DTO 一致；stdout 无日志污染。
- daemon restart 后历史可读、read 自动 load 但不重发、write 显式 task.continue 才恢复且不自动续跑。

## 8. Phase 4：Conversation UI

交付：

- Vue stores 与 snapshot + mutation subscription。
- 完整窗口：history、task header、timeline、Plan bar、composer、settings。
- popover 紧凑渲染，同源 timeline。
- reasoning preview/summary/Markdown、tool type renderers、terminal ANSI、diff、context notice。
- 所有 disclosure 的稳定 key/三态 expansion、跨窗口同步与持久化。
- bottom-follow 状态机、stable detached anchor、unread/jump-to-latest、selectionEpoch 隔离、virtualized history/timeline。
- i18n zh-CN/en、dark/light、reduced motion、keyboard/a11y。
- 独立 diagnostic view，默认隐藏。

测试：

- reducer fixture 的 DOM 顺序与无 raw JSON 断言。
- streaming Markdown、XSS、external link、remote image policy。
- manual scroll/resize/expand state 测试。
- 10,000 item 虚拟列表、内层 scroller 不 detach、popover reopen stable anchor、聚合 prepend/merge/split anchor、A→B→A 与同 task 重订阅 race、跨 WebView ui_state generation/revision。
- popover 与 full window 同步。
- follow-up 只在 session idle 时可发，复用同一 session。

## 9. Phase 5：托盘、窗口与 Agent 集成

交付：

- Tauri tray、左键 popover、右键原生菜单、跨平台坐标/clamp/fallback。
- `off|active|always` 生命周期与三平台登录项。
- Codex TOML / Claude JSON CST 最小编辑、status/update/remove。
- Settings Integrations/General/History/Diagnostics UI。
- CLI `agents` 与 `setup/app` 单实例路由。

测试：

- 配置编辑 golden tests：其它字节/语义不变，非法文件不覆盖，重复 install 无 diff。
- outdated path/timeout 检测。
- tray mode 与 login item 纯函数/平台 adapter 测试。
- 当前平台手动验证 click positioning、focus-loss hide、输入焦点、窗口唯一性。

## 10. Phase 6：迁移、文档与发布

交付：

- 删除旧 `plugins/grok-codex`、Node scripts、旧 test、localhost dashboard doc 与旧 package scripts。
- README：安装、Grok 前置、MCP 集成、使用、read/write 安全、UI、历史、doctor、开发、测试、卸载旧 plugin。
- CI release matrix、artifact/checksum。
- license attribution：借鉴设计不复制 Zed GPL 代码；从 MIT/Apache 项目复制的实质代码需保留相应 notice。
- `docs/grok-activity-app.md` 删除或改为 migration note，不能继续描述现行架构。

验收：运行 `docs/acceptance.md` 的全部自动和人工清单。

## 11. Grok 实施批次

为降低单次上下文和 review 风险，按以下批次委托：

1. Phase 0–1：骨架、持久化、IPC。
2. Phase 2–3：ACP、daemon、CLI、MCP。
3. Phase 4：对话 UI。
4. Phase 5–6：托盘、集成、迁移、发布。

每个批次必须：

- 开始前读取完整规格，但只改本批次范围；
- 结束时列出变更文件、设计偏差、测试命令与结果；
- 不提交、不推送，除非用户另行授权；
- 不修改 `/Users/qian/project/AskHuman`；
- 不调用另一个 coding agent 替它写代码。

## 12. Codex review 门槛

每批次 review 顺序：

1. 检查工作树与范围，识别用户已有修改。
2. 阅读所有新增/修改代码，不只看测试结果。
3. 运行格式、lint、typecheck、unit、integration、build。
4. 针对进程生命周期、race、权限、路径、序列化、持久化和 UI 状态做 adversarial review。
5. 将 findings 按 P0/P1/P2 写给 Grok，要求逐条修复并补回归测试。
6. 重跑受影响测试和全量门槛。

任何以下情况阻止进入下一阶段：

- 测试失败或被跳过而无说明；
- 主视图可见 raw ACP JSON；
- mode 存在默认或自动推断；
- daemon/GUI/MCP 各有独立 ACP 状态源；
- localhost/外部浏览器成为运行依赖；
- 用户手动展开或滚动仍会被自动覆盖；
- Windows/Linux 只是 cfg stub，不能编译或没有明确降级行为。
