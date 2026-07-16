# GrokTask CLI 与 MCP 契约

状态：已确认的实现规格。

## 1. 命令总览

```text
GrokTask --help
GrokTask --version
GrokTask doctor
GrokTask setup
GrokTask app [--task TASK_ID]

GrokTask mcp

GrokTask run --mode read|write --cwd PATH [--model ID] [--effort VALUE] TASK...
GrokTask start --mode read|write --cwd PATH [--model ID] [--effort VALUE] [--submission-id UUID] TASK...
GrokTask status TASK_ID [--json]
GrokTask wait TASK_ID TURN_ID [--timeout SECONDS] [--json]
GrokTask cancel TASK_ID (--turn TURN_ID | --recovery RECOVERY_ID) [--json]

GrokTask tasks list [--limit N] [--json]
GrokTask tasks show TASK_ID [--json]
GrokTask tasks clear [--inactive-only]

GrokTask agents status [codex|claude]
GrokTask agents mode codex|claude none|mcp

GrokTask daemon run
GrokTask daemon start|stop|restart [--force]|status|logs
```

隐藏内部入口：`GrokTask --gui-host`。除内部登录项与进程启动器外，不在帮助中宣传。

## 2. CLI 通用行为

- `--help`、`--version` 在本进程完成，不启动 daemon。
- `run/start` 的 `mode` 与 `cwd` 必填；cwd 解析为绝对、真实存在的目录后才提交。
- task 可以来自剩余 argv；支持 `--prompt-file PATH` 与 stdin 的增强可后续加入，首发至少实现 argv。
- 正常机器输出使用 stdout；日志和警告使用 stderr。
- `--json` 输出单个 JSON object，不混入进度日志。
- 人类可读 `run` 在完整完成时输出最终 Markdown；partial 时先输出明确的部分结果提示。元数据摘要走 stderr 或显式 `--json`。

退出码：

| code | 含义 |
| --- | --- |
| 0 | 请求有效且本轮 completed/cancelled，或幂等管理命令成功 |
| 1 | 参数、配置、集成编辑或本地 I/O 错误 |
| 2 | task failed、Grok 不可用、未登录或协议失败 |
| 3 | daemon/IPC 异常中断 |

## 3. MCP server

`GrokTask mcp` 使用 stdio，只在 stdout 发送 MCP framing；所有日志写 stderr/daemon log。server name 为 `groktask`，不发布 UI resource、localhost URL 或 MCP Apps 模板。

工具集合固定为六个：

- `run`
- `start`
- `continue`
- `status`
- `wait`
- `cancel`

`continue` 是阻塞式 follow-up：在同一 `taskId` 上调用 daemon `task.continue`，协议层复用已持久化的 ACP `sessionId`（`session/load` + `session/prompt`），返回新 turn 的不可变 `RunResult`。主机策略上用于**相关且健康**的实现 follow-up 或审查驱动修复；上下文陈旧/污染/不相关、会话不健康，或需要干净实现上下文时，主机可改用 `run`/`start` 新开（用户明确重置足够但不强制）。桌面 UI 仍可直接使用内部 `task.continue`。

### 3.1 公共任务输入

```ts
type TaskInput = {
  task: string;              // trim 后非空，最多 200,000 UTF-8 bytes
  cwd: string;               // 必须是绝对、存在的目录
  mode: "read" | "write";  // 必填，无默认
  model?: string;            // 非空、最多 128 bytes
  effort?: string;           // 直接映射 --reasoning-effort，最多 64 bytes
  title?: string;            // 可选 UI 标题，最多 160 chars
};

type StartInput = TaskInput & {
  submissionId: string;       // caller-generated UUID，异步 start 的持久化幂等键
};
```

MCP `start` 使用 `StartInput` 并要求 submissionId；`run` 仍使用 TaskInput。CLI 未传 `--submission-id` 时生成 UUID 并在 accepted 输出中返回，显式重试时可复用。未知字段按 MCP schema 拒绝或忽略必须保持一致；推荐 schema `additionalProperties: false`。

### 3.2 `run`

用途：启动一个 client-owned 任务并阻塞到 turn 完成。

输入：`TaskInput`。

结构化输出：

```ts
type RunResult = {
  taskId: string;
  turnId: string;
  turnOrdinal: number;
  status: "completed" | "cancelled" | "failed";
  mode: "read" | "write";
  sessionId?: string;
  requestedModel?: string;
  actualModel?: string;        // spawn/init 前失败时可能未知
  stopReason?: string;
  turnOutcome: "completed" | "partial" | "refused" | "cancelled" | "failed";
  partial: boolean;
  answer: string;             // 公开 assistant segments 按时序拼接的 Markdown
  error?: {
    code: string;
    message: string;
    retryable: boolean;
  };
  startedAt: string;          // RFC 3339
  finishedAt: string;
  durationMs: number;
};
```

MCP text content 为一段简洁摘要：`completed && !partial` 时以 `answer` 为主；`partial` 时必须以“部分结果（达到 token/turn 上限）”开头再给出已有 answer；failed/cancelled 时给出 taskId、turnId 与错误/取消原因。这样忽略 `structuredContent` 的 client 也不会把 partial 当成完整 final。相同数据同时放入 `structuredContent`，不得返回完整 transcript 或原始 ACP JSON。

调用方 stdio 断开，或 MCP client 对该 `run` request 发送 `notifications/cancelled` 后，只取消该 request 绑定的 client-owned `(taskId, turnId)`；不能只取消 Rust handler future、不能取消后来由 UI 发起的另一 turn，也不能留下原 Grok prompt 运行。

### 3.3 `start`

用途：启动 daemon-owned 异步任务并立即返回。

输入：`StartInput`。

输出：

```ts
{
  submissionId: string;
  taskId: string;
  turnId: string;
  turnOrdinal: number;
  status: "queued" | "starting";
  mode: "read" | "write";
  createdAt: string;
  taskDeleted?: boolean;       // 24h dedupe 命中但用户已清除 task 时为 true
}
```

MCP 进程断开不取消异步任务。

### 3.4 `continue`

用途：在已有 task 上追加一轮用户 prompt，阻塞到新 turn 完成。不创建新 task，不改 mode。

输入：

```ts
{
  taskId: string;           // 同一主机对话 + workspace 上 run/start 返回并保留的 id
  prompt: string;           // 非空，上限同 task
  timeoutMs?: number;       // 可选；默认与 wait 上限一致（max 300_000）
}
```

行为：

1. 调用 daemon `task.continue` 创建新 turn（ordinal N+1）；
2. 若 task 已有 `acp_session_id`，ACP 运行时 `session/load` 该 id，再 `session/prompt`（禁止 follow-up 上 `session/new`）；
3. 阻塞等待该 turn 的不可变 `RunResult`（与 `run`/`wait` 相同 shape）。

复用或新开由 Codex / Claude Code 主机判断：相关且健康的实现 follow-up 用 `continue`；工作不相关、上下文陈旧/污染、会话不健康/空响应/不收敛、mode/workspace 边界，或干净上下文更安全时可用 `run`/`start`。用户明确 reset 足够但不是必要条件；Grok 不决定会话生命周期。不得静默把 task 的 `read` 改成 `write`。

### 3.5 `status`

输入：

```ts
{ taskId: string }
```

输出：

```ts
type TaskStatus = {
  taskId: string;
  status: "queued" | "starting" | "running" | "cancelling" |
          "recovering" | "idle" | "cancelled" | "failed" | "interrupted";
  mode: "read" | "write";
  sessionState?: "warm" | "cold" | "unavailable";
  activeTurnId?: string;
  activeRecoveryId?: string;
  lastTurnId?: string;
  lastTurnStatus?: "completed" | "partial" | "refused" | "cancelled" | "failed";
  actualModel?: string;
  currentStep?: string;
  latestAction?: string;
  answerPreview?: string;
  stopReason?: string;
  error?: { code: string; message: string; retryable: boolean };
  createdAt: string;
  updatedAt: string;
  finishedAt?: string;
};
```

它是快照读取，不阻塞、不返回全量 timeline。

### 3.6 `wait`

输入：

```ts
{
  taskId: string;
  turnId: string;
  timeoutMs?: number; // default 30,000; min 0; max 300,000
}
```

指定 turn 在窗口内完成时返回同一个不可变 `RunResult`。`wait` 不得把 `turnId` 解释为调用时的 latest/current；T1 完成后即使 UI 已启动 T2，重复 wait(T1) 仍返回 T1。超时不是 MCP error，返回：

```ts
{
  taskId: string;
  turnId: string;
  timedOut: true;
  status: TaskStatus["status"];
  currentStep?: string;
  latestAction?: string;
}
```

客户端可以重复 wait 同一 `(taskId, turnId)`。wait 调用断开不影响 daemon-owned 任务。

### 3.7 `cancel`

输入是严格互斥 union：`{ taskId, turnId }` 取消一个 Turn，或 `{ taskId, recoveryId }` 取消 task-scoped recovery。ID 是 compare-and-cancel 边界；旧 ID 绝不能误取消后来一轮/恢复。

输出：

```ts
type TurnCancelResult = {
  target: "turn";
  taskId: string;
  turnId: string;
  taskStatus: TaskStatus["status"];
  alreadyTerminal: boolean;
  result: RunResult;
};

type RecoveryCancelResult = {
  target: "recovery";
  taskId: string;
  recoveryId: string;
  taskStatus: TaskStatus["status"];
  alreadyTerminal: boolean;
  recoveryStatus: "completed" | "cancelled" | "failed";
  error?: { code: string; message: string; retryable: boolean };
};

type CancelResult = TurnCancelResult | RecoveryCancelResult;
```

Turn cancel 等待状态对应的有界流程结束，并总是返回目标 Turn 的不可变 `RunResult`；无法确认退出则 result 为 failed/`cancel_timeout`。目标 turn 已终态时不改变任何状态，直接返回该 result 与 `alreadyTerminal: true`，同时 `taskStatus` 可以合法地是后来 T2 的 running，不能用 task 状态伪造 T1 结果。Recovery cancel 中止 load/process、固化 recovery row，Task 回 interrupted 或 failed，不改写 last Turn。taskId 与 target ID 不匹配或不存在返回标准 MCP invalid params/not found error。

### 3.8 MCP request cancellation

- daemon 为每次 `run` 保存运行时 binding `(connectionId, requestId) -> (taskId, turnId)`，并在 `turns` 中持久化 owner kind。收到 `notifications/cancelled` 或连接断开时，只有该 turn 尚未终态才发 conditional `task.cancel(taskId, turnId)` 并等待有界收尾；迟到 cancellation 在 T1 结束、T2 开始后不能影响 T2。
- `start` 在 daemon accepted 之前被取消时撤销 submission；accepted 后已经是 daemon-owned，MCP request cancellation 不终止它，调用方应显式使用 `cancel`。
- `status`/`wait` 被取消只结束本次读取/等待，不改变 task。
- `cancel` request 被取消只停止等待响应；已提交给 daemon 的 conditional Turn/recovery cancel 不回滚。
- stdio 整体断开等价于取消该连接仍在执行的所有 `run`，但不影响已 accepted 的 `start`。

## 4. 错误分类

在创建任务前即可判断的错误使用 MCP protocol error：

- 缺少 mode/cwd/task，或 start 缺少 submissionId；
- cwd 不存在、不是目录或不是绝对路径；
- model/effort/title 超限；
- taskId/turnId/recoveryId/submissionId 格式非法、不存在或不属于同一个 task。

任务创建后的运行错误返回正常工具结果中的 `status: failed`：

- `grok_not_found`
- `grok_not_authenticated`
- `grok_version_unsupported`
- `acp_initialize_failed`
- `session_create_failed`
- `session_load_failed`
- `session_unavailable`
- `grok_process_exited`
- `daemon_interrupted`
- `read_mode_violation`
- `permission_unavailable`
- `agent_refusal`
- `unexpected_stop_reason`
- `task_timeout`
- `storage_failure`
- `cancel_timeout`
- `internal_error`
- `idempotency_conflict`

错误 message 必须可行动，包含下一步建议，但不得泄露 token、完整环境或未脱敏协议 payload。

## 5. MCP 工具描述要求

`run/start` description 必须明确：

- 任务会发送给外部 xAI Grok 服务；
- 主机完成分析/plan 后，委派范围仅为**已规划的实现**：写/改代码、文件修改、测试与修复执行（实现执行器）；**不得**把调试/根因、code review、研究、性能/稳定性/安全分析宣传为 Grok 职责；
- prompt 应携带 plan/spec/诊断与验收标准；
- `write` 可修改传入 cwd；
- mode 必须由调用方根据用户意图显式选择；后续不得静默 read→write；
- 进度始终本地持久化；用户打开应用或启用 `active/always` 托盘时可实时查看；
- `run` 等待最终答复，`start` 适合后台任务；
- 主机可选择 `run`/`start` 以获得干净/新上下文（工作不相关、上下文陈旧/污染、先前会话不健康、mode/workspace 边界需要时）；`taskId` 可在后续相关健康 follow-up 时供 `continue` 使用。

`continue` description 必须明确：传入已有 `taskId` + 新 prompt；用于相关健康的实现 follow-up 或主机审查驱动修复；调用 `task.continue` 而非新建 task；阻塞返回新 turn 的不可变结果；不改变 mode；主机也可改选 `run`/`start` 新开（用户明确重置足够但不强制）。

`status` 的 description 必须要求传回原 taskId；`wait` 必须传回 start/run/continue 对应的 taskId 与 turnId；`cancel` 传 turnId 或 status 给出的 activeRecoveryId。`start` description 要求调用方为一次逻辑提交生成并在 retry 时复用 submissionId。不得在描述中暗示 GrokTask 会提交、推送或绕过工作目录范围。

## 6. CLI 与 MCP 一致性

- CLI `run/start/status/wait/cancel` 与 MCP 共用 daemon IPC；MCP `continue` 组合 `task.continue` + `task.wait`。
- CLI JSON 输出与 MCP structuredContent 使用同一 Rust DTO 和序列化测试。
- taskId、turnId、状态枚举、error code、answer 拼接规则完全一致。
- CLI 与 MCP 同时等待同一 `(taskId, turnId)` 时，只订阅状态，不重复启动 Grok。

## 7. `doctor`

`GrokTask doctor` 只读检查：

- GrokTask version 与当前 executable path；
- Grok CLI path/version；
- `grok models` 是否可调用及默认模型；
- 本地 Grok 登录/ACP initialize 是否可用；
- daemon 与 SQLite 健康；
- GUI/托盘能力；
- Codex/Claude MCP 集成状态：not installed / installed / outdated / invalid config。

默认不启动真实 prompt、不修改配置。`--json` 便于自动诊断。

## 8. 长任务超时

- MCP 集成写入 24 小时 tool timeout，使 `run` 不被客户端默认一分钟超时截断。
- GrokTask 自身默认单 turn hard runtime 为 4 小时，可在设置中调整 5 分钟至 24 小时。
- 达到 hard runtime 后记录本地 `terminationCause=hard_timeout`，再按 cancel 流程终止；即使 ACP 最终返回 `stopReason: cancelled`，结果仍按优先级返回 failed/`task_timeout`。
