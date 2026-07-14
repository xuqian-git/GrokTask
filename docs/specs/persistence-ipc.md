# GrokTask 持久化与本地 IPC 规格

状态：已确认的实现规格。

## 1. 本地目录

沿用 AskHuman 的可预测单目录模式：

```text
~/.groktask/
  config.json
  history.sqlite3
  history.sqlite3-wal
  history.sqlite3-shm
  daemon.lock
  daemon.json
  daemon.sock              # Unix only
  gui-host.lock
  gui-host.sock            # Unix only
  daemon.log
  gui.log
```

Windows endpoint 使用 named pipe，不创建 `.sock`：

```text
\\.\pipe\groktask-daemon-<current-user-sid-hash>
\\.\pipe\groktask-gui-<current-user-sid-hash>
```

目录与文件只允许当前用户访问。数据库、配置和日志都不得放入项目工作区。

## 2. 配置

`config.json` 使用 versioned schema，缺失字段取默认，未知字段保留。写入采用同目录临时文件、fsync 和 atomic rename；解析失败时停止修改并保留原文件。

首发字段：

```json
{
  "schemaVersion": 1,
  "general": {
    "language": "system",
    "theme": "system",
    "trayMode": "active",
    "historyLimit": 200,
    "maxConcurrentTasks": 3,
    "taskTimeoutSeconds": 14400,
    "grokExecutable": null
  },
  "ui": {
    "popoverWidth": 420,
    "popoverHeight": 620,
    "showDiagnostics": false
  }
}
```

约束：

- historyLimit：0–5000；0 表示不长期保留已结束 conversation，但每轮结束后持久化的 30 分钟 `retentionProtectUntil` 与 window/request lease 期间仍可 follow-up。daemon 退出或 warm child 提前 eviction 不缩短该 deadline。
- maxConcurrentTasks：1–8。
- taskTimeoutSeconds：300–86400。
- grokExecutable：null 或存在的绝对文件路径。
- trayMode：`off | active | always`。

Daemon 监听原子替换和普通写事件，去抖后 reload；无效新配置只记录错误并继续使用最后一份有效配置。GUI 保存后显示 daemon 是否已应用。

## 3. SQLite 模式

数据库使用 WAL、foreign_keys=ON、busy_timeout，并由单一 storage actor 串行执行 schema migration 和写事务。

### 3.1 `tasks`

核心字段：

```text
id TEXT PRIMARY KEY
title TEXT NOT NULL
cwd TEXT NOT NULL
mode TEXT NOT NULL
status TEXT NOT NULL
session_state TEXT             # warm | cold | unavailable
recovery_state TEXT            # none | pending | loading | failed | manual_required
active_recovery_id TEXT
requested_model TEXT
actual_model TEXT
reasoning_effort TEXT
grok_version TEXT
acp_protocol_version INTEGER
acp_session_id TEXT
last_turn_id TEXT
supervisor_pid INTEGER
supervisor_started_at INTEGER
daemon_instance_id TEXT
stop_reason TEXT
error_code TEXT
error_message TEXT
created_at INTEGER NOT NULL
started_at INTEGER
updated_at INTEGER NOT NULL
finished_at INTEGER
retention_protect_until INTEGER
last_sequence INTEGER NOT NULL
timeline_generation INTEGER NOT NULL DEFAULT 1
state_revision INTEGER NOT NULL DEFAULT 1
```

`tasks.status` 只使用 `queued|starting|running|cancelling|recovering|idle|cancelled|failed|interrupted`。`tasks.finished_at/stop_reason/error_*` 只是最近一轮/容器列表的可覆盖 mirror；新 follow-up 开始时按状态更新，历史结果绝不能从这些字段重建。每轮不可变时间与结果以 `turns` 为准。Task 没有单一 owner，多轮 owner 属于 `turns`。

### 3.2 `turns`

```text
id TEXT PRIMARY KEY
task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE
ordinal INTEGER NOT NULL
prompt_markdown TEXT NOT NULL
status TEXT NOT NULL
owner_kind TEXT NOT NULL       # client | daemon
owner_connection_id TEXT       # opaque live-daemon binding, nullable
owner_request_id TEXT          # opaque MCP/IPC request binding, nullable
mode TEXT NOT NULL
session_id TEXT
requested_model TEXT
actual_model TEXT
answer_markdown TEXT NOT NULL DEFAULT ''
stop_reason TEXT
termination_cause TEXT
partial INTEGER NOT NULL DEFAULT 0
error_code TEXT
error_message TEXT
error_retryable INTEGER
result_json TEXT               # terminal canonical RunResult, then immutable
created_at INTEGER NOT NULL
started_at INTEGER
prompt_dispatched_at INTEGER   # set only after session/prompt request write_all succeeds
finished_at INTEGER
UNIQUE(task_id, ordinal)
```

`turns.status` 只使用 `queued|starting|running|completed|partial|refused|cancelled|failed`。`interrupted` 只属于可恢复的 Task container：daemon crash 时旧活动 Turn 立即以 `failed/daemon_interrupted` 固化，保证 wait(turnId) 有不可变结果，Task 再进入 interrupted 等待 load/retry。Turn drain 的同一 transaction 写入所有 result 字段与 canonical `result_json`；此后永不随下一轮或 task mirror 字段变化。历史 `wait(taskId,turnId)` 直接反序列化该 row，并用 DTO migration 升级旧 schema。运行时 cancel map 以 `(connectionId, requestId) -> (taskId, turnId)` 为权威；持久化 owner 字段用于 crash recovery 与审计，旧 daemon 的 connection binding 不得在重启后复用。

### 3.3 `recovery_operations`

Task-scoped load/resume 不是 Turn，单独持久化：

```text
id TEXT PRIMARY KEY            # client operationId or daemon UUID
task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE
action TEXT NOT NULL           # resume | send | retry_interrupted | auto_resume
input_hash TEXT NOT NULL
prompt_markdown TEXT            # only send; encrypted-at-rest is out of scope like turns
status TEXT NOT NULL           # pending | running | completed | cancelled | failed
expected_last_turn_id TEXT NOT NULL
created_turn_id TEXT           # only send/retry after successful load
error_code TEXT
error_message TEXT
result_json TEXT
created_at INTEGER NOT NULL
started_at INTEGER
finished_at INTEGER
```

同一 task 同时最多一个 active recovery。Cancel/restart/crash 只改变 recovery row 与 Task container，绝不重写 `expected_last_turn_id` 指向的 immutable Turn。

### 3.4 `submissions`

异步 MCP `start` 的 exactly-once 接受记录：

```text
submission_id TEXT PRIMARY KEY # caller-generated UUID
input_hash TEXT NOT NULL       # canonical TaskInput excluding submissionId
task_id TEXT NOT NULL
turn_id TEXT NOT NULL
accepted_result_json TEXT NOT NULL
created_at INTEGER NOT NULL
expires_at INTEGER NOT NULL
```

同一 submissionId + 相同 input hash 重试直接返回原 accepted result，不创建第二个 task；相同 ID + 不同输入返回 `idempotency_conflict`。记录与 task/turn 创建在同一 transaction 提交，提交前不叫 accepted。去重记录不含 prompt，只保留 24 小时且不随 task clear 立即级联，避免 response 丢失后因历史删除而把 retry 变成第二个任务；若目标 task 已被用户清除，仍返回原 IDs 并标记 `taskDeleted: true`。

### 3.5 `timeline_items`

当前 materialized snapshot：

```text
task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE
item_id TEXT NOT NULL
turn_id TEXT
kind TEXT NOT NULL
first_sequence INTEGER NOT NULL
last_sequence INTEGER NOT NULL
payload_json TEXT NOT NULL
created_at INTEGER NOT NULL
updated_at INTEGER NOT NULL
PRIMARY KEY(task_id, item_id)
```

Active Plan 不是第二份表：它是 `timeline_items(kind='plan')` 中带稳定 originSequence 的 materialized anchor，payload lifecycle 为 `active_hidden | completed_visible | cleared`。subscribe header 的 `activePlanItemId` 指向当前 `active_hidden` row，完整 row 仍走 snapshot chunk；turn drain 的单个 transaction 原位更新为 `completed_visible` 并追加一个携带完整 payload/originSequence 的 `plan_finalize` mutation。前端把“移除 active bar + 在旧位置显示历史卡”作为一个 store commit，因此状态条与历史卡来自同一权威数据且不会短暂重复/消失。

### 3.6 `timeline_mutations`

追加式 UI/调试事件：

```text
task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE
sequence INTEGER NOT NULL
generation INTEGER NOT NULL
operation TEXT NOT NULL       # add | update | remove | plan | plan_finalize | state | reset
item_id TEXT
payload_json TEXT NOT NULL
created_at INTEGER NOT NULL
PRIMARY KEY(task_id, sequence)
```

### 3.7 `raw_acp_events`

经过 bounded 与 redact 的诊断流：

```text
task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE
raw_sequence INTEGER NOT NULL
direction TEXT NOT NULL       # to_agent | from_agent | stderr | lifecycle
method TEXT
payload_json TEXT NOT NULL
created_at INTEGER NOT NULL
PRIMARY KEY(task_id, raw_sequence)
```

### 3.8 `ui_state`

```text
task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE
disclosure_key TEXT NOT NULL
expansion TEXT NOT NULL       # user-expanded | user-collapsed
revision INTEGER NOT NULL
updated_at INTEGER NOT NULL
PRIMARY KEY(task_id, disclosure_key)
```

`auto` 不必落盘；没有 row 即为 auto。数据库 metadata 另持久化随机 `ui_state_generation` UUID 与单调 `ui_state_revision`；daemon 重启延续 revision，数据库重建/恢复时 generation 改变，仍存活的 GUI host 必须清空旧 expansion cache 并 resnapshot。

## 4. 写入与广播顺序

每个 ACP 输入导致的写操作必须在一个 SQLite transaction 内完成：

1. 用独立的 `raw_sequence` 写 redacted raw event。
2. 更新 `timeline_items` / task / turn / plan。
3. 为 reducer 产生的每个可见 mutation 分配单独、连续的 `timeline sequence`，追加 `timeline_mutations` 并推进 task.last_sequence；一个 ACP 输入可以产生零个、一个或多个 mutation。
4. commit。
5. commit 成功后才向 IPC subscribers 广播 mutation。

这样 GUI 收到的 sequence 必定能从数据库重放。commit 失败时不广播，task 进入 storage failure；不得让 UI 看见无法恢复的虚假状态。

允许将 16ms 内的文本 chunk 合并到一次 transaction，但类型边界前必须 flush。

## 5. 历史保留

- 默认保留最近 200 个 task，按 `created_at` 计算。
- 每个 turn 到终态、session eviction 和 daemon 空闲时执行 retention。
- 候选是没有活动 turn 的 idle/cancelled/failed task。queued/starting/running/recovering/cancelling/interrupted、`retention_protect_until > now`、持有可见 window/request lease 或 warm child 的 task 不删除。
- 超出 historyLimit 时，先对最旧 idle task 关闭 warm child并转 cold，再按 created_at 删除最旧无 lease/保护 deadline 的 task。historyLimit=0 也等待持久化的 30 分钟 continuation deadline；daemon 5 分钟 idle exit 只把 session 变 cold，不提前删除。
- 删除 task 通过 foreign key cascade 清理 turn、timeline、raw event 与 ui state。
- “清空历史”是显式删除流程，不绕过安全边界：storage actor 只对无 window/request lease 的候选取得 deletion guard/tombstone（不是公开 TaskStatus）；已有 lease/request 的 task 返回 skipped/busy。guard 生效后拒绝一切新的 task-scoped lease 与方法，包括 run/status/wait/cancel/continue/snapshot/subscribe、ui_state.get/set 与诊断读取。对 idle warm task 再关闭 supervisor并确认 process tree 退出，撤销残余 subscription、cascade delete、广播 `task.deleted`。所有 task 方法都必须在同一 actor 内满足“先取得 request lease或发现 tombstone”，不能先读 row 后补 lease。与 clear 并发的操作要么在 guard 前取得 lease并让 clear skip，要么在 guard 后失败，不能写入已删除 FK。包含活动 turn 仍需明确确认、逐 turn cancel并确认终态后，等 lease drain 再进入上述流程；clear result 列出 deleted 与每个 skipped reason。
- 执行 retention 后按阈值 checkpoint WAL；不在每次删除后 VACUUM。设置中提供“压缩数据库”维护动作。

## 6. 数据边界与隐私

历史数据库有意保存用户 prompt、Grok reasoning、公开回复、工具语义详情和 bounded 输出，因为这是产品的完整本地 transcript 功能。

以下内容在进入 `raw_acp_events` 和日志前脱敏：

- 常见 API key、Bearer token、Authorization header、cookie、OAuth secret；
- 环境变量中 key 名匹配 `TOKEN|SECRET|PASSWORD|API_KEY|PRIVATE_KEY` 的值；
- Grok 登录凭证与 MCP credential；
- URL query 中常见 credential 参数。

日志不记录完整 prompt/reasoning/tool output，只记录 taskId、method、size、status 和错误摘要。数据库 UI 明确提示历史可能包含代码与思考内容，并提供清空功能。

单个 payload 与 item 按 ACP runtime 限制截断；截断必须写入 `{truncated: true, originalBytes, preview}` 语义，UI 可见。

容量预算：raw diagnostics 每 task 64 MiB/100,000 events、全库 512 MiB；normalized timeline 每 task 256 MiB。全库 raw 超限时只从最旧无活动 task 的 raw rows 开始删除，并在 task 留下 `raw_pruned` marker，永不删除其 normalized transcript。数据库超过 2 GiB 时先执行 raw prune、history retention 与 WAL checkpoint，并向 Settings/doctor 报告；不得静默清除仍在 historyLimit 内的 normalized task。`daemon.log` 每文件 20 MiB，保留 3 个轮转文件。

## 7. IPC transport

- 编码：UTF-8 NDJSON，一行一个 JSON object。
- Unix：Tokio UnixStream；socket mode 0600。
- Windows：Tokio named pipe；ACL 只允许当前 user SID。
- frame 默认上限 8 MiB；空行跳过；解析错误返回 protocol error 后关闭连接。
- 客户端与服务端都有 bounded outbound queue；慢 GUI 客户端超限后断开并要求重连 snapshot，不能阻塞 ACP actor。
- GUI host 使用一条轻量 control connection，并为每个 committed/pending task subscription 建立独立 data connection；一个 connection 只承载一个 snapshot/live stream。A live、pending B 大 snapshot、popover 与完整窗口因此不会共享 FIFO 产生 head-of-line blocking。stream 结束/取消就关闭对应 data connection；daemon 仍通过各连接广播相关 ui_state change。

## 8. Handshake

客户端首帧：

```json
{
  "type": "hello",
  "requestId": "...",
  "protocolVersion": 1,
  "role": "mcp|cli|gui-host",
  "clientVersion": "0.1.0",
  "binaryPath": "/absolute/path/GrokTask",
  "binaryFingerprint": { "size": 123, "mtimeNs": 456 },
  "pid": 1234
}
```

Daemon：

```json
{
  "type": "hello_ack",
  "requestId": "...",
  "protocolVersion": 1,
  "daemonVersion": "0.1.0",
  "status": "ok|restarting|replacement_deferred|incompatible",
  "reason": null,
  "retryUntil": null
}
```

protocolVersion 不兼容时不得继续解释业务消息。binary fingerprint 不同但 protocol 兼容时走 graceful replacement。`restarting` 必须给出 RFC 3339 `retryUntil`；`replacement_deferred` 表示旧 daemon 已恢复接单，client 继续使用旧 daemon，并把更新延后作为可行动状态报告，不再盲目重启循环。旧 client 尚不认识该 hello status 时，daemon 也通过普通 `replacement_deferred` business error 保持可诊断。

## 9. 请求/响应 envelope

```ts
type Request = {
  type: "request";
  requestId: string;
  method: string;
  params: unknown;
};

type Response = {
  type: "response";
  requestId: string;
  ok: boolean;
  result?: unknown;
  error?: { code: string; message: string; retryable: boolean };
};
```

业务 method 至少包含：

```text
task.run
task.start
task.status
task.wait
task.cancel
task.continue          # GUI internal
task.snapshot
task.subscribe
task.unsubscribe
lease.acquire
lease.renew
lease.release
ui_state.get
ui_state.set
tasks.list
tasks.clear
settings.get
settings.update
health.get
```

MCP `run/start/status/wait/cancel` 是这些 request 的薄适配。

### 9.1 Turn 与 continue 契约

- `task.start` accepted response 与 `task.run` 内部 accepted event 都包含创建时分配的 `turnId`；`task.wait/task.cancel` 必须同时传 `taskId,turnId`。
- `task.continue` 仅供 GUI，输入 `{ operationId, taskId, expectedLastTurnId, action, prompt? }`，其中 caller-generated operationId 是持久化幂等键，action 为 `resume | send | retry_interrupted`。`resume` 只 load session、不创建 turn/不重发 prompt；`send` 要求非空新 prompt；`retry_interrupted` 明确复制最近 `failed/daemon_interrupted` turn 的旧 prompt，但创建新的 daemon-owned turnId。同 operationId/输入重试返回原 recovery/result，不重复 load 或创建 turn；不同输入返回 conflict。
- `idle` 可执行 send；`interrupted` 可执行三种 action，先原子转 recovering 并 load，成功后 resume 回 idle，或为 send/retry 创建新 turn 再 queued/running。load 失败回 interrupted 并保存可行动错误；不会把旧 turn 改回 running。`expectedLastTurnId` 不匹配时返回 conflict，防止双击/另一窗口启动新 turn 后重复发送。
- read 的启动恢复 scheduler 创建 daemon UUID 的 `auto_resume` recovery，但绝不自动执行 retry；write 必须由用户触发 continue。GUI 请求一旦获得 accepted 新 turn response，该 turn 为 daemon-owned，窗口断开不取消。

### 9.2 Lease 契约

- window lease key 为 `(connectionId, leaseId)`，scope 为 `daemon` 或 `task:<taskId>`，默认 TTL 60 秒；`lease.acquire/renew/release` 幂等，renew 只能由原 connection 完成。连接断开立即回收，异常未检测到时最迟 TTL 回收。
- 可见 history/settings shell 需要 daemon 时取得 daemon scope；打开具体 task 时不先单独 acquire，而由首次 `task.subscribe` 的 `lease` 参数在同一 storage barrier 原子取得 task scope，再判断 retention 并捕获 snapshot。窗口每 30 秒 renew，隐藏/关闭 release。
- `task.run/status/wait/cancel/continue/snapshot/subscribe`、`ui_state.get/set` 与 task-scoped diagnostic read 在 router 接受 request 时自动取得 task-scoped request lease，response/snapshot stream 结束或连接断开时释放；lease acquisition、tombstone check 与 task lookup 由 storage actor 串行，避免 lookup 前被 retention 删除。空闲 MCP connection 本身没有 lease。
- 一个 leaseId 的 scope 不可改变，每次 subscribe 都使用全新 leaseId，包含同 task S1→S2。A→B/S1→S2 时，旧 stream/lease 继续保护 committed 画面；pending stream 在独立 data connection 上每 30 秒续租。Snapshot deadline 为 45 秒，短于 60 秒 window TTL；snapshot 校验成功后必须先经 storage actor 立即 renew 新 lease 获得完整 60 秒，再原子 promote，随后才 unsubscribe/release旧 lease。失败或 stale 时只 unsubscribe/release pending lease，旧 committed lease 不动。popover 隐藏时取消 current/pending stream 并释放全部 task lease。

## 10. 增量订阅

GUI 请求（`afterSequence` 与 `generation` 可省略；省略即要求完整 snapshot）：

```json
{
  "method": "task.subscribe",
  "params": {
    "taskId": "...",
    "surfaceId": "popover-main",
    "selectionEpoch": 42,
    "subscriptionEpoch": 9,
    "streamId": "client-generated-uuid",
    "afterSequence": 120,
    "generation": 3,
    "lease": { "leaseId": "...", "ttlMs": 60000 }
  }
}
```

`streamId` 由 GUI host 在发请求前生成并全局唯一，daemon 只回显；因此 header 尚未返回时，A→B→C、隐藏或 data connection 异常也能由 control connection 立即 `task.unsubscribe`。关闭该 data connection 同样自动取消 producer/backlog/read transaction，并回收其 request/window lease；不得留下等待 deadline 的 orphan stream。

订阅采用严格 barrier，但绝不能在发送大 snapshot 时暂停 Task actor。Storage actor 在一个很短的串行 cutover 中：

1. 原子取得/续期请求中的 task-scoped window lease，若 task 正在 deletion guard 下则失败；
2. 用独立 SQLite read connection 开启 WAL read transaction并立即读取 `B = lastSequence`、timeline generation `G`、持久化 UI state generation `UG` 与 revision `U`，从而固定 snapshot@B/U；
3. 在下一次 commit/broadcast 前注册该 subscriber 的有界 ordered channel fence；以后所有 timeline `>B` mutation 与 UI state `>U` change 都 non-blocking 写入该 channel；
4. 把 subscribe response/header 先写入 connection FIFO，然后立即让 Task actor 继续处理 ACP。独立 snapshot producer 持有 read transaction 并负责读取/发送历史。

每个 subscriber channel 默认上限为 10,000 event 或 16 MiB（先到者）。Task actor 只做 non-blocking send；超限时终止该 stream、释放 read transaction 并要求该 subscriber 重连，不能阻塞其它 subscriber、storage commit 或 Grok stdout drain。Snapshot phase 有 45 秒交付 deadline；到期只断该 stream并释放 read transaction，成功进入 live phase 后不受这个 snapshot timer 限制。Snapshot producer 先发送 frozen snapshot，再从同一个 channel 按产生顺序发送积压与后续 live event，因此 `>B` 永远位于 `snapshot_end` 之后且不丢不重。

当 client generation 与当前 generation 相同，且 mutation log 仍覆盖 `afterSequence + 1..B`，daemon 可以返回 delta header：

```json
{
  "mode": "delta",
  "task": {},
  "activePlanItemId": "plan:...",
  "taskId": "...",
  "surfaceId": "popover-main",
  "selectionEpoch": 42,
  "subscriptionEpoch": 9,
  "generation": 3,
  "fromSequence": 120,
  "lastSequence": 135,
  "uiStateGeneration": "...",
  "uiStateRevision": 7,
  "streamId": "...",
  "timelineEntryCount": 15,
  "uiStateRowCount": 4
}
```

否则返回在同一 barrier B 上读取的完整 snapshot：

```json
{
  "task": {},
  "activePlanItemId": "plan:...",
  "taskId": "...",
  "surfaceId": "popover-main",
  "selectionEpoch": 42,
  "subscriptionEpoch": 9,
  "generation": 3,
  "lastSequence": 135,
  "uiStateGeneration": "...",
  "uiStateRevision": 7,
  "streamId": "...",
  "timelineEntryCount": 10000,
  "uiStateRowCount": 4
}
```

`activePlanItemId` 只是一项小引用；完整 active plan row 与其它 materialized items 一样走 chunk/fragment，避免大 Plan 塞进 header。无论 timeline 使用 full 或 delta，每次 subscribe 都在同一 read transaction 中发送该 task 在 revision U 的完整 `ui_state` rows，避免另一次 get/subscribe 间隙。Header 后由 stream worker 发送带完整 envelope 的 frame：

```ts
type SnapshotFrameBase = {
  type: "event";
  taskId: string;
  surfaceId: string;
  selectionEpoch: number;
  subscriptionEpoch: number;
  streamId: string;
};

type SnapshotChunk = SnapshotFrameBase & {
  event: "task.snapshot_chunk";
  chunkKind: "timeline_items" | "timeline_mutations" | "ui_state_rows";
  chunkIndex: number;
  entries: unknown[];
};
```

普通 `task.snapshot_chunk` 编码后不得超过 1 MiB。单个规范化 item 允许达到 ACP runtime 的 2 MiB 上限，因此超过 chunk budget 时改发 `task.snapshot_item_fragment`：将该 entry 的 canonical JSON UTF-8 bytes 切为不超过 700 KiB 的 raw fragment，base64 编码，携带 `entryId, fragmentIndex, fragmentCount, totalBytes, sha256`；客户端只在 staging 中校验并重组，绝不展示半个 item。最后发送 `task.snapshot_end`，包含相同 routing fields、G/B/UG/U、item/fragment counts 与校验摘要。

snapshot stream 已经包含所有 `sequence <= B` 与 UI state `revision <= U` 的效果，客户端不得再应用该范围 change。客户端先在以 `(surfaceId,taskId,selectionEpoch,subscriptionEpoch,streamId)` 为 key 的 staging store 组装并校验，收到 end 后一次性 commit；中途断线、fragment/hash/count 不匹配就丢弃 staging 并重订阅。只有完整 tuple 匹配 committed 或 pending selection 时才可应用/待 promote。迟到 stream 不能改可见 store；后台 timeline 与 UI-state 是两个独立缓存域，分别按 `(timelineGeneration,lastSequence)` 与 `(uiStateGeneration,uiStateRevision)` 单调更新，不能用 timeline 未变的旧 UI rows 覆盖新展开状态。generation UUID/epoch 变化只接受当前 daemon handshake + 当前 subscription 的 reset，stale stream 无权引入旧/未知 generation。

`snapshot_end` 后同一个 forwarding worker 才发送 channel 中的 live envelope：

```json
{
  "type": "event",
  "event": "task.mutation",
  "taskId": "...",
  "surfaceId": "popover-main",
  "selectionEpoch": 42,
  "subscriptionEpoch": 9,
  "streamId": "...",
  "sequence": 136,
  "payload": {}
}
```

Chunk、fragment、end、task.mutation 与 ui_state.changed 每一帧都必须通过当前五元组检查；unsubscribe 竞态中已经进入 socket 的旧 live frame 也要丢弃，不能只在 snapshot commit 时检查。

`task.unsubscribe { taskId,surfaceId,streamId,selectionEpoch,subscriptionEpoch }` 幂等取消 in-flight producer、丢弃该 stream backlog，并阻止后续 frame；它不隐式释放 lease，客户端按上面的 transfer/hidden 顺序另调 `lease.release`。客户端发现 sequence gap、timeline generation 变化或 UI state generation 变化时停止应用增量、递增 subscriptionEpoch 并在新的 data connection 重订阅。每条 data connection 的 response/event 共用自己的有序 writer queue，每个 stream 的 worker 是该连接唯一写入者；control 与其它 surface/stream 不进入该 FIFO。

ACP load reconciliation 需要替换可重放范围时，在单个 SQLite transaction 内更新 materialized rows、递增 `timeline_generation`、追加一个 `reset` mutation 并 commit；commit 后广播 `task.timeline_reset { generation, sequence }`。已连接 GUI 收到后丢弃旧 generation 的可见缓存并重新 subscribe；在新 snapshot 到达前不展示逐项 remove/add 中间态。transaction 失败则 generation 与旧 timeline 完全不变。

`ui_state.set { taskId,disclosureKey,expansion,mutationId }` 由 storage actor transactionally upsert/delete `(taskId,disclosureKey)`，采用 server-serialized last-write-wins，在 metadata 推进持久化 `uiStateRevision` 并把 revision 写入 row；commit 后广播 `ui_state.changed { taskId,disclosureKey,expansion,uiStateGeneration,revision,mutationId }`。订阅 fence 确保该 task 的 revision `>U` change 排在 snapshot_end 后；两个 WebView 按 generation/revision 忽略旧回声，发起方用 mutationId 对账。revision 是全库 counter，因此某个 task 的事件可跳号，client 不把跳号当丢包。`ui_state.get` 只用于非 live 诊断；可见 surface 必须使用 task.subscribe barrier。scroll/follow/draft 不使用该通道。

`plan_finalize` 是一种特殊 timeline mutation：payload 带完整 plan row、originSequence 和 itemId。前端在一个 store/virtual-list transaction 中移除 active projection、插入或显现历史 item，并用 detached anchor 补偿旧位置插入；不得拆成普通 remove + update。

## 11. GUI host IPC

GUI host 自有 endpoint 用于单实例导航：

```text
gui.open_popover
gui.open_task { taskId }
gui.open_history
gui.open_settings
gui.focus
gui.quit
```

CLI 找到现有 host 时发送 command；找不到则 spawn `--gui-host`，握手后重试。GUI host 连接 daemon 获取数据，不缓存第二份权威 task state。

## 12. 恢复与迁移

- 每次 schema migration 在 transaction 中执行并记录版本；失败时备份数据库路径并拒绝启动 task，不自动删除用户历史。
- daemon 每次启动生成 `daemon_instance_id`。任务开始时记录 supervisor pid 与可核对的 process start identity；正常 supervisor 由匿名 control pipe 保证 daemon 死亡即清理 Grok 树。
- 正常 daemon idle exit/replacement 在关闭 control pipe 前，用一个 transaction 把本实例所有 `idle + session_state=warm` task 改为 cold，并清空 supervisor pid/start identity/daemon instance ownership；随后关闭/确认 child。Crash startup 对 `session_state=warm` 但 daemon_instance_id 不匹配的 idle task 先核对旧 supervisor 已退出（匹配时终止，不匹配 PID 不发信号），再执行同样 cold/clear 修复。新 daemon 绝不 attach stale transport，warm LRU 只统计本实例实际持有的 live child。
- 启动恢复按崩溃前状态分支：`cancelling` 必须确认旧 supervisor/control pipe 已退出，绝不 load，并按持久化 `termination_cause` 完成为 cancelled 或原 policy/timeout failure；`starting/running` task 的旧活动 turn 固化为 `failed/daemon_interrupted`，task container 在可恢复时变 interrupted；`recovering` 没有活动 Turn，当前 recovery operation 固化为 `failed/daemon_interrupted`、last Turn 保持不变，task 回 interrupted；daemon-owned `queued` turn 可重新入队，client-owned `queued` turn 因 owner connection 消失转 cancelled。若仍发现 start identity 完全匹配的孤儿 supervisor，先终止并确认退出；identity 不匹配时不得向复用 PID 发信号，只报告 recovery error。
- 只有已持久化可 load/resume 的 ACP sessionId 才进入 interrupted recovery；若 crash 发生在 session 建立前，旧 turn 仍 failed/daemon_interrupted，但 task 直接 failed/session_unavailable，绝不猜测 prompt 是否已发送后自动重发。read task 的 recovery scheduler 仅在 recovery_state 不是 `failed|manual_required` 时自动执行 `interrupted -> recovering` 并尝试 load 上下文/历史；成功后转 idle、显示“旧 prompt 未重发”notice，只有用户点击 retry 才创建新 turn。load 失败回 interrupted、`recovery_state=failed`，普通 composer 保持禁用但允许显式 resume/retry。
- write task 启动后保持 interrupted，绝不自动 load 或继续写。用户明确调用 `task.continue(resume|send|retry_interrupted)` 时才 `interrupted -> recovering`；resume 成功只到 idle，send/retry 成功创建全新 daemon-owned turn。任何路径都不把旧 interrupted turn 改回 running。
- `timeline_items` 是跨 generation 的权威 materialized snapshot；`timeline_mutations` 用于当前 generation 的增量订阅与审计，不承诺单靠跨 reset 日志重建全量。reset 是 compaction/reconciliation 边界，旧 generation mutation 可按保留策略删除。启动健康检查验证 tasks.lastSequence/current generation 与 mutation 尾部一致，并验证 materialized rows 可解码。
