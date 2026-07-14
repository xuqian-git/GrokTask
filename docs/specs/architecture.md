# GrokTask 系统架构

状态：已确认的实现规格。

## 1. 技术栈

- 桌面与打包：Tauri 2。
- 核心运行时：Rust + Tokio。
- UI：Vue 3 + TypeScript + Vite。
- MCP server：Rust `rmcp` stdio transport。
- ACP：官方 `agent-client-protocol` Rust crate（v1 能力面）与 Tokio transport；未知扩展字段仍保留到诊断层。
- 数据：SQLite（bundled SQLite，WAL 模式）+ JSON 配置。
- Markdown：前端解析、禁用原始 HTML并做 URL 安全过滤。

生产构建必须启用 Tauri `custom-protocol` 并嵌入前端资源，不依赖 Vite dev server。

## 2. 单二进制多角色

```text
Codex / Claude Code
        |
        | stdio MCP
        v
  GrokTask mcp             GrokTask CLI
   瘦客户端                 瘦客户端
        \                    /
         \ NDJSON IPC      /
          v                v
     GrokTask daemon run  <-------------------+
       每用户单实例                             |
       无 Tauri / 无 WebView                    | IPC event subscription
       TaskManager                              |
       ACP session actors                       |
       SQLite history                           |
          |                                     |
          | spawn supervisor + control pipe     |
          v                                     |
     GrokTask --task-supervisor                 |
          | proxy stdin/stdout JSON-RPC         |
          v                                     |
     grok ... agent stdio                       |
                                                |
     GrokTask --gui-host -----------------------+
       每用户单实例 Tauri 事件循环
       tray + popover + main/settings windows
```

所有角色来自同一个 `GrokTask` 可执行文件：

- `mcp`：stdio MCP server；不初始化 Tauri，只把工具请求转换为 daemon IPC。
- CLI：本地命令；不初始化 Tauri，除非请求打开应用或设置。
- `daemon run`：任务与 ACP 的唯一权威；无 GUI。
- 隐藏 `--gui-host`：Tauri 单实例，承载托盘、浮层、完整窗口和设置窗口。
- 隐藏 `--task-supervisor`：每个 Grok 进程的极小守护/stdio proxy；daemon 控制管道 EOF 时终止 Grok process group/job，防 daemon 崩溃留下孤儿。

## 3. Daemon 职责

Daemon 是唯一可以：

- 创建、恢复、继续和取消 ACP session；
- 维护任务状态机与每 session 串行 prompt 队列；
- 写入任务历史与诊断日志；
- 向 MCP、CLI 和 GUI 广播规范化事件；
- 执行历史保留与进程清理；
- 暴露当前版本、实际 Grok 版本和健康状态。

MCP 和 GUI 不得各自启动 Grok，也不得实现第二套 ACP reducer。

## 4. Task 与 Session actor

每个任务有一个 actor，actor 独占：

- task state；
- Grok 子进程与 JSON-RPC transport；
- ACP `sessionId`；
- 当前 prompt request ID；
- 当前 `turnId` 与该 turn 的 owner binding；
- 可选 active `recoveryId`（与 Turn 互斥）；
- prompt FIFO；
- event sequence counter；
- normalized timeline reducer；
- cancel token 与 drain timer。

Actor 不直接裸 spawn Grok，而是 spawn 同二进制 supervisor。Supervisor 建立新的 Unix process group 或 Windows Job Object（kill-on-close），启动 Grok，透明代理 stdin/stdout/stderr，并监听只继承给本进程的匿名 control pipe。Daemon 正常退出、崩溃或被 kill 导致 control EOF 时，supervisor 必须 TERM/KILL Grok 树后自身退出。这样新 daemon 不需要采用身份不明的旧进程。

同一 session 同时最多执行一个 `session/prompt`。普通 UI follow-up 只有在 task container 为 `idle` 且 session 可恢复时才能入队；`interrupted` 的唯一入口是 conditional `task.continue`：`resume` 只完成 load 并回到 idle，`send/retry_interrupted` 才在 load 成功后原子创建新 turn。不同任务可以并发运行；默认最大运行数为 3，其余任务进入 FIFO 队列，该值可配置为 1–8。

## 5. 状态机

```text
queued
  -> starting
  -> running
  -> idle          follow-up -> running

任一活动态 -> cancelling -> cancelled | failed
任一活动态 -> failed

进程重启后的可恢复任务：interrupted -> recovering -> idle | interrupted | failed
恢复操作取消：recovering -> interrupted | failed
```

定义：

- Task 是可包含多轮的 conversation container；Turn 才有 `completed | partial | refused | cancelled | failed` 终态。
- `idle` 表示最近一轮已经结束、当前没有 prompt，session 可以 warm 或 cold；历史列表根据 lastTurnStatus 显示“已完成/部分完成/已拒绝”。
- 每个 Turn 在创建时就分配不可复用的 `turnId` 与 ordinal。MCP `run/start/wait` 与 Turn cancel 绑定明确的 `(taskId, turnId)`；无 Turn 的 load/resume 使用独立 `(taskId,recoveryId)`。GUI follow-up 和 request cancellation 不能在实现中临时解释为“该 task 最新操作”。`run/wait` 返回的是它发起或等待的 Turn result，不把 task container 永久锁成 completed。显式 follow-up 会把 idle task 转回 running并创建新 turn。
- 终态不因迟到的 ACP notification 被重新改成 running；迟到内容只在 drain 窗内归入刚结束的 turn。

Idle session 资源有界：Grok 子进程在 turn 结束后默认 warm 30 分钟，最多保留 3 个 idle process，超出按 LRU 关闭；关闭前持久化 sessionId。cold idle task 保留历史，下次 follow-up lazy spawn + `session/load`。Daemon 退出时关闭全部 warm process，不影响可恢复历史。

Warm process TTL 与历史保留是两个概念。每轮结束时同时持久化 `retentionProtectUntil = finishedAt + 30 分钟`；daemon 提前退出、LRU eviction 或 child 变 cold 都不能缩短这个 deadline。`historyLimit=0` 也必须在该 deadline 与所有 lease 结束后才可删除，因此 daemon 5 分钟 idle exit 不会让用户失去已承诺的 30 分钟继续窗口。

## 6. Daemon 生命周期

- MCP/CLI 无法连接时，以 detached 方式启动 `GrokTask daemon run`，等待握手后重试。
- daemon 使用用户级 lock 保证单实例；持锁后才清理 stale endpoint。
- 协议版本或二进制指纹不一致时，只有不存在 queued/starting/running/cancelling Turn 与 active recovery operation、没有尚未完成的 `run/wait/cancel` request，且 accepted `start/continue` 的 response 已经 `write_all + flush` 到仍存活的 IPC connection（submissionId/operationId、状态与结果均已持久化，可幂等 retry/status 找回），旧 daemon 才可完成 endpoint/lock handoff 后退出。否则进入 drain：拒绝新的 run/start/continue，但继续 status/wait/cancel 与既有结果交付；最多等待 10 分钟。届时仍不安全则放弃 replacement、恢复旧 daemon 接单，并通过兼容旧连接的 hello/business error 报告 `replacement_deferred`，不得因自动换新取消任务。
- 无活动/排队/恢复任务、无可见窗口 lease、无进行中的 IPC request 且持续 5 分钟后，daemon 可以退出。空闲的长驻 MCP stdio 进程和隐藏的 `always` GUI host 连接本身不构成 keepalive；下次 tool/UI 操作会重新拉起并连接 daemon。
- 存在异步任务时，即使发起方断开也不得空闲退出。
- daemon 崩溃时，旧活动 Turn 立即固化为 `failed/daemon_interrupted`，阻塞 `run` 与任何 wait(turnId) 都得到结构化终态；若当时只有 recovery operation，则只把 recovery 标 failed/daemon_interrupted，last Turn 不变。Task container 才进入 interrupted。启动后的新 daemon 可以为具有 ACP session ID 的 task 恢复会话上下文与历史，但绝不自动重发崩溃前未完成的 prompt。read task 由 recovery scheduler 自动 load 到 idle；只有用户明确 retry 才创建新 turn。write task 保持 interrupted，必须由用户显式 resume/send/retry 触发 `task.continue`，尤其不得自动 load 后继续写操作。

显式 `daemon restart` 与自动 replacement 使用完全相同的 result-delivery barrier：存在 queued/starting/running/cancelling Turn、active recovery、尚未完成的 run/wait/cancel，或 accepted start/continue response 尚未对活连接完成 `write_all + flush` 时，默认拒绝/等待并列出 taskId + turnId/recoveryId；仅进入进程内 writer queue 不算安全。连接已断时，生效状态必须已持久化且操作可由 submissionId/operationId/status 幂等恢复。只有 `--force` 才按相应 Turn/recovery cancel 流程终止操作、让现有 request 收到结果，然后 handoff。握手收到 `restarting` 的 client 使用 100–1000ms jitter backoff：自动 replacement 的既有 client 跟随旧 daemon 最长 10 分钟 drain deadline，而显式 restart 的无活动 handoff 最多等待 30 秒；deadline 由 hello 中的 `retryUntil` 给出，不使用互相矛盾的本地常量。`replacement_deferred` 时继续使用协议兼容的旧 daemon 并向用户报告更新延后。旧 daemon 持 lock 时关闭 listener、写 handoff 状态再退出；新进程只有拿到 lock 后才清理 stale endpoint。必须测试旧进程在 listener-close 与 lock-release 之间崩溃的恢复。

## 7. GUI host 与托盘生命周期

托盘设置为：

- `off`（默认）：daemon 不自动启动 GUI host；CLI `app` 或显式打开任务时仍可启动完整窗口，但不创建 tray，最后一个窗口关闭后 GUI host 退出。
- `active`：任务进入 queued/running 时启动 GUI host；最后一个任务结束后保持到 daemon 的 5 分钟空闲 grace，若用户正在查看窗口则由窗口 lease 延长；随后 GUI host 与 daemon 退出。不因空闲 MCP 连接常驻；不安装登录项。
- `always`：安装用户登录项并常驻 GUI host；daemon 按任务需要启动，GUI host 可显示离线/未运行状态。

GUI host 与 daemon 生命周期独立：`always` host 隐藏常驻不阻止 daemon idle exit；off/active/always 下任何可见 task/history 窗口每 30 秒续租一个 60 秒 connection-scoped window lease，窗口隐藏/关闭后显式 release，崩溃/断线则立即回收或最迟 TTL 到期。打开具体 task 时，首次 subscribe 必须在同一个 storage barrier 内先取得 task-scoped lease 再做 retention/snapshot；不能靠“GUI 已连接/已订阅”推断可见。Settings 中不依赖 daemon 的页面不续租。always host 在打开任务、刷新或发送 follow-up 时按需唤醒 daemon并重连。

GUI host 单实例。再次运行 `GrokTask app`、点击设置或打开某任务，只向已存在 host 发送导航命令并聚焦对应窗口。

左键托盘图标：切换 popover。右键：原生菜单，包含当前任务摘要、打开 GrokTask、历史、设置、daemon 状态/重启、退出。

popover 是无边框 Tauri WebViewWindow：

- 使用 tray click event 的屏幕坐标与尺寸锚定，并裁剪到 monitor work area；
- macOS 位于菜单栏图标下方，Windows 位于任务栏托盘上方；Linux 优先使用事件坐标，缺失时用当前显示器右上角回退；
- 点击外部时隐藏，但任务继续；
- 输入 follow-up 时允许获得键盘焦点；
- 每次打开根据 tray event 的物理坐标、目标 monitor scale factor 和 work area 换算为 Tauri 逻辑坐标；多显示器/DPI 变化后重新计算，work area 小于常规最小尺寸时启用更小的紧凑布局；
- 某些 Linux StatusNotifier/AppIndicator host 不提供可靠左键 activate event 时，原生菜单的“打开当前任务/打开浮层”是等价入口，`doctor` 报告该能力降级；
- 隐藏 popover 时保留草稿并停止抢占键盘；再次打开恢复 composer 内部焦点，但不在隐藏状态抢回此前应用焦点；
- 尺寸与完整窗口分别持久化。

## 8. 前后端边界

前端只接收：

- task snapshot；
- timeline add/update/remove；
- plan snapshot；
- usage/model/state update；
- connection health；
- UI action result；
- shared UI state snapshot/change（展开意图）。

前端不得解析 ACP `sessionUpdate` 或 `_x.ai` 通知。所有语义化和去重都发生在 Rust reducer。

Tauri commands 只连接 GUI host 本地 IPC；不直接持有 daemon 的内部对象。GUI host 对 daemon 保持轻量 control connection，并为每个 committed/pending subscription 使用独立 data connection，避免大历史 snapshot 阻塞其它窗口 live/control。窗口通过 storage barrier subscribe：actor 只原子捕获 snapshot@B、注册 subscriber fence 与 lease，独立 snapshot producer 再从 SQLite read transaction 分块读取；actor 继续 drain ACP。每个 subscriber 的有界 ordered backlog 暂存 `B+1...`，只在 `snapshot_end` 后由同一 forwarding worker 顺序发送；超限只断开该 subscriber。generation reset 强制原子 resnapshot，避免间隙、重复和 load 中间态。

Daemon/storage actor 是展开意图的持久化所有者：WebView 通过 `ui_state.set` write-through，commit 后广播带持久化 generation/revision 的 `ui_state.changed`。task.subscribe 在同一 barrier 返回 UI rows@U，保证新 surface 不漏 get 与 broadcast 之间的更新。revision 对某个 task 可以跳号（其它 task 也共享全局 counter），只要求严格递增；scroll/follow/unread/draft 由 GUI host 按 surface/task 留在内存，不进入共享 SQLite。

## 9. 建议模块边界

```text
src-tauri/src/
  main.rs
  cli/
  mcp/
  daemon/
    lifecycle.rs
    task_manager.rs
    task_actor.rs
  acp/
    process.rs
    client.rs
    reducer.rs
    normalize.rs
    drain.rs
  ipc/
    codec.rs
    protocol.rs
    transport.rs
  storage/
    db.rs
    migrations.rs
    repository.rs
    retention.rs
  integrations/
  app/
    gui_host.rs
    tray.rs
    windows.rs
  config.rs
  paths.rs

src/
  views/PopoverView.vue
  views/TaskView.vue
  views/HistoryView.vue
  views/SettingsView.vue
  components/timeline/
  stores/
  lib/ipc.ts
  lib/markdown.ts
```

模块名可按实现语言惯例微调，但职责和依赖方向不可反转。

## 10. 崩溃与故障隔离

- 单个 Grok 进程失败只终止对应任务，不杀 daemon。
- 单个前端窗口或 GUI host 退出不影响任务。
- SQLite 写失败时停止接收新任务，并向现有客户端报告 degraded；不得继续运行却丢失审计记录。
- 未识别 ACP 通知记录为 bounded diagnostic，不得 panic。
- 无效 JSON-RPC、超大行、非法 UTF-8 或 stderr 洪泛必须有大小限制并产生可理解错误。
- daemon 退出时先取消或持久化任务状态，向 Grok 发送合作式取消，再有界 TERM/KILL，并清理子进程。

## 11. 跨平台约束

- Unix 使用 Unix domain socket，权限 0600。
- Windows 使用 named pipe，并限制为当前用户 SID。
- 路径、命令和工作目录始终作为结构化参数传递，禁止经 shell 拼接。
- Windows 子进程使用新的 process group；Unix 使用 process group，使取消能够覆盖 Grok 的子进程树。
- Linux 无托盘或图形会话时，MCP/CLI/daemon 仍需完整工作，只把 GUI 状态报告为 unavailable。
