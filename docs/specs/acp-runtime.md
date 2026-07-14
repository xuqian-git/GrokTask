# GrokTask ACP 运行时规格

状态：已确认的实现规格。协议基线为 ACP v1；实现必须按能力协商，不把 Grok CLI 的普通 `--resume` 等同于 ACP `session/resume`。

## 1. Grok 进程启动

每个新任务由 daemon 使用参数数组直接 spawn，不经过 shell：

```text
grok --no-auto-update <mode args> [--model MODEL] [--reasoning-effort EFFORT] agent stdio
```

工作目录通过 `Command::current_dir(cwd)` 设置。Grok 可执行文件默认从 PATH 解析，也允许用户在设置中指定绝对路径；解析后的实际路径、版本和参数摘要进入任务元数据，但环境变量值不得记录。

### Read 参数

```text
--sandbox read-only
--permission-mode dontAsk
--disable-web-search
--no-subagents
--allow Read
--allow Grep
--deny Edit
--deny WebFetch
```

### Write 参数

```text
--sandbox workspace
--always-approve
--deny "Bash(git push*)"
--deny "Bash(git commit*)"
--deny "Bash(git clean*)"
--deny "Bash(git reset --hard*)"
--deny "Bash(gh pr*)"
--deny "Bash(rm -rf*)"
```

根据 [xAI Sandbox 与 Permissions 文档](https://docs.x.ai/build/enterprise#sandbox)，Grok sandbox 默认是 `off`，因此 write 绝不能只依靠 process cwd。`workspace` 将写入限制为 cwd、`/tmp` 与 `~/.grok/`，并始终保护常见 credential 目录。上述 deny 在 `--always-approve` 下仍优先，是针对常见直接命令的 defense-in-depth；Grok 的 shell、alias 或网络调用可能绕过 glob，不能把它宣传成不可绕过的 no-commit/no-push 安全边界。GrokTask 自身绝不调用这些操作，UI 明确 write 风险。read 模式依赖 Grok `dontAsk` 自带的安全 shell/git fast path，不使用 `Bash(git *)` 这种会放行 push/reset/clean 的规则。

`mode` 是 MCP `run/start` 的必填枚举；缺失或未知值在 spawn 之前返回 invalid-argument。不得因为任务文本包含“修改”“review”等词自动推断模式。

## 2. 初始化与能力协商

启动后严格执行：

1. 建立逐行 JSON-RPC transport，注册 notification/request router。
2. 发送 `initialize`，声明 GrokTask 实际完整支持的 client capabilities。首发不声明 fs 或 terminal capability。
3. 如果 agent 要求 ACP authentication，按其返回的方法完成已有本地登录态验证；不收集或代理用户凭证。
4. 新任务调用 `session/new`；恢复任务仅在能力允许时调用 `session/load` 或 `session/resume`。
5. 保存 `sessionId` 后才发送 `session/prompt`。

本机 Grok 0.2.101 的实测能力：

- `loadSession: true`；
- `sessionCapabilities` 为空；
- 因此允许 `session/load`，禁止假设存在 `session/resume/list/delete/close`；
- 同一进程内多轮对话通过重复 `session/prompt` 完成。

任何 omitted capability 都按不支持处理。

## 3. Prompt 生命周期

### 3.1 发送

- 每个 session 使用 FIFO 串行 prompt 队列。
- prompt 文本去除全空白后必须非空，UTF-8 长度默认上限 200,000 字节。
- `session/prompt` 返回前，实际内容通过 `session/update` 流式进入 reducer。
- prompt response 只作为 stop metadata；不得把 response 当最终回复文本。

### 3.2 完成与迟到事件

收到 prompt response 后：

1. 记录 `stopReason`，但暂不立即关闭 turn。
2. 继续接收迟到 notification，直到连续 500ms 没有该 session 的可见更新。
3. hard cap 为 5 秒；到达后强制 flush。
4. 先按本地 `terminationCause` 优先级，再按下面的 stopReason 映射标记最后一个公开 assistant segment；若不存在，answer 为空并保留 stopReason。
5. 当前工具仍未终态时标记为 `unknown` 或 `cancelled`，不能永久显示 running。

该 drain 窗只归并已经开始的 turn；下一条 follow-up 在 turn 完成后才出队。

StopReason 映射：

| ACP stopReason | Turn 状态 / MCP 结果 | UI 与 session 行为 |
| --- | --- | --- |
| `end_turn` | `completed` / status completed | 最后一段标记 `finalAnswer`，task 转 idle，可 follow-up |
| `max_tokens` | `partial` / status completed + `partial: true` | 最后一段标记 `partialAnswer`，显示达到 token 上限 notice，可 follow-up |
| `max_turn_requests` | `partial` / status completed + `partial: true` | 显示达到 turn 上限 notice，可 follow-up |
| `refusal` | `refused` / status failed + `agent_refusal` | 显示明确拒绝，保留本地用户消息但标记“未进入 agent 上下文”；task 转 idle，可重新措辞 |
| `cancelled` | `cancelled` / status cancelled | 未完成 item 标 cancelled，不作为最终回复 |
| 未知值 | `failed` / `unexpected_stop_reason` | 保留内容与诊断，关闭该 session process，禁止直接 follow-up 直到 load/new |

不得把 partial/refusal 文本伪装成正常 final answer。Turn 状态持久化在 `turns.status`，Task 在无 prompt 且 session 可用时回到 `idle`。

`terminationCause` 是 daemon 在发出 `session/cancel` 前持久化的本地权威原因，优先于 ACP 最终常见的 `stopReason: cancelled`：

| terminationCause | 最终结果 |
| --- | --- |
| `user_cancel | mcp_cancel | client_disconnect | restart_force` | cancelled |
| `read_mode_violation` | failed / `read_mode_violation` |
| `permission_unavailable` | failed / `permission_unavailable` |
| `hard_timeout` | failed / `task_timeout` |
| `cancel_timeout` | failed / `cancel_timeout` |

只有 `terminationCause` 为空时才使用通用 StopReason 表。cancel drain 中收到的 ACP cancelled 只证明 prompt 已停，不能覆盖 policy/timeout 原因；对应 precedence 必须有 fixture。

## 4. 严格时序 reducer

Reducer 是 ACP 原始流到 UI/持久化语义流的唯一转换层。

### 4.1 通用规则

- 每个输入事件先分配单调递增 `ingestionSequence`；每个可见 add/update mutation 再分配独立的 `timelineSequence`。一个 ACP 事件可能在边界 flush 时产生多个 mutation，不能强迫它们共用一个序号。
- 所有可见 item 有稳定 `itemId`；标准 ID 优先使用 `messageId/toolCallId`，否则使用持久化的本地 ID。
- add 与 update 分开广播，并按 `timelineSequence` 严格排序。update 永远不改变 item 在时间线中的原始位置。
- 任何不同的可见语义类型（reasoning、assistant、tool、plan、permission、context notice）到达前都必须 flush 当前连续文本 segment。即使 Plan 主要投影到底部活动条，它仍是 segment 边界。
- 原始事件先 bounded、redact 后写诊断层；只有已识别语义进入正常时间线。
- streaming buffer 每个 animation frame 最多向 UI flush 一次；在类型边界前同步 flush，保证顺序。

### 4.2 `agent_thought_chunk`

- 如果当前开放 segment 是同 turn 的 reasoning，则追加文本。
- 否则先结束当前开放 segment，在当前位置新增 `reasoning_segment`。
- Tool、Plan、assistant text 或 prompt 完成会结束当前 reasoning segment。
- Grok 当前没有标准 messageId 时，连续 thought chunks 以边界规则合并，不能每 token 生成卡片。

### 4.3 `agent_message_chunk`

- 如果当前开放 segment 是同 turn 的 assistant text，则追加。
- 否则先 flush 上一个 segment，再新增 `assistant_segment`。
- Tool、Plan、permission、context notice 或 reasoning 都可以打断 assistant text；之后到来的 text 创建新的 segment，不能移动到所有动作之后。
- turn drain 后按 StopReason/terminationCause 映射处理最后一个公开 assistant segment：仅 `end_turn` 标 `finalAnswer`，limit 类标 `partialAnswer`，refusal/cancelled/failed 不标 final；UI 不复制第二份。

### 4.4 `tool_call` 与 `tool_call_update`

- 新 `tool_call` 在到达位置创建一张卡，key 为 `toolCallId`。
- 后续 update 合并到同一 item；scalar 只在出现时覆盖，collection 字段按 ACP 语义整体替换。
- update 先于 create 到达时创建 placeholder，并在后续 create 原位补齐。
- 主标题优先级：`title` → 内容中首个非空文本首行 → 唯一 location path → 人类化 `kind + tool name`。
- command、path、diff、terminal output、raw input/output 解析成类型化详情；解析失败时正常视图仍显示 title/status，原始值只进诊断层。
- 已完成的相邻轻量 read/search 可以在渲染层聚合，但 reducer 和持久化仍保留独立 item；编辑、终端、错误与权限永不聚合。

### 4.5 `plan`

- 每个通知是当前完整快照，不是 delta。
- 首个 plan 为当前 turn 创建一个带 `originSequence` 的稳定 plan anchor；active plan 按 session/turn 原位替换，状态条只投影最新 snapshot。
- active 时 anchor 不渲染成重复卡片。ACP v1 没有 planId，且 completed entries 后仍可能继续更新，因此同一 turn 的所有 plan snapshot 都更新同一个 anchor，只在 turn drain 完成时用一个 `plan_finalize` mutation 把它原子转成不可变、可见的 `plan_snapshot`，保持首次 plan 的原始位置并同时移除 active bar。新 turn 才创建新 anchor。

### 4.6 其他标准更新

- `user_message_chunk`：live prompt 可能回显，也可用于 load replay。live 时与本地预插入的当前 turn 用户消息按 messageId/promptId 和文本 prefix 对账，更新同一 item或忽略完全相同 echo，绝不复制；load 时按 messageId/replay turn 边界重建。
- `usage_update`：更新任务 header/metadata，不进对话流。
- `current_mode_update`、`config_option_update`、`session_info_update`：更新 session metadata；只有会影响用户判断的变化才生成简短 context notice。
- `available_commands_update`：更新能力缓存，不进对话流。
- permission request：先 flush 当前 segment，在到达位置创建稳定 `permission:<turnId>:<requestId>` 语义 item，并关联到等待中的 tool card。JSON-RPC requestId 只在当前 transport 内唯一，但 turnId 在 transport crash 后不会复用，因此组合不会与恢复后的新 turn 碰撞。item 只显示人类化动作与结果，不显示 request/options JSON；生命周期是 `requesting -> allowed_once | rejected | cancelled`，tool card 同步显示相同 substatus。它是必须回应的 JSON-RPC request，不能只记录 UI 状态：
  - read mode：立即选择最严格可用的 reject option（优先 reject_once，其次 reject_always）；若没有 reject option则返回 cancelled。随后任务以 `read_mode_violation` 失败。
  - write mode：用户已显式授权 write，立即选择 `allow_once`；绝不选择 allow_always。没有 allow_once 时返回 cancelled，并以 `permission_unavailable` 失败。
  - task 已 cancelling/cancelled：立即返回 cancelled。
  - handler 内部响应期限为 2 秒；异常仍必须发送 cancelled response，再结束任务。不得弹出脱离上下文的全局权限框。

read 拒绝或 write 无可用 allow_once 时，必须先持久化对应 `terminationCause`、回应 permission request、把 permission item 更新为 rejected/cancelled，再进入统一 `session/cancel` + soft wait + supervisor TERM/KILL 流程。确认原 prompt 不再运行后才把 turn 标 failed。Agent 在取消 drain 内发出的事件仍可落诊断，但不得把已失败 turn 重新变为 running。

### 4.7 xAI 扩展通知

- `_x.ai/*`、hook、queue、settings、summary 和 completion 的原始方法名/payload 默认只进诊断层；这不等于丢弃其中所有信息。
- 可以消费其中明确、稳定且经过测试的 model、promptId、eventId、chunkId、isReplay 和 usage 字段，但不得因扩展缺失破坏 ACP 主流程。
- Rust normalizer 可以从经过 fixture 锁定的 xAI 扩展提取阶段标题、工具语义文本或 terminal 关联信息，再输出标准 timeline item；前端永远不接触扩展 payload。未知扩展只进诊断层。
- `eventId/chunkId` 可用于去重；它们不是标准 ACP，必须有无扩展字段的回退测试。

## 5. 流式文本缓冲

- 收到 ACP chunk 后立即追加到后端 canonical text。
- GUI 增量推送按约 16ms 合并，遇到类型边界、prompt response 或 cancel 时立即 flush。
- 前端显示可以用 `requestAnimationFrame` 平滑揭示积压文本，目标在约 200ms 内追上后端，不得改变 canonical text。
- 字符切分按 Unicode scalar/grapheme 安全边界；不得截断中文、emoji 或组合字符。
- Markdown 使用同一个 item 原位更新，不销毁/重建包含滚动和展开状态的父组件。

### 5.1 稳定 ID

- task 与 turn 使用持久化 UUID。
- 标准 messageId 存在时：`msg:<sessionId>:<messageId>:<kind>`。
- 标准 messageId 缺失时：`seg:<turnId>:<segmentOrdinal>:<kind>`；`segmentOrdinal` 按该 turn 的严格边界顺序递增，在 segment 首次创建时立即落盘。
- tool：`tool:<sessionId>:<toolCallId>`，不能使用跨 session 的裸 toolCallId。
- plan：`plan:<turnId>`；ACP v1 无 planId，同一 turn 只有一个可更新 anchor。
- permission：`permission:<turnId>:<requestId>`；关联 toolCallId 只作为链接，不能替代 request ID。
- load staging 使用 replay 的用户消息顺序映射持久化 turn ordinal，再按标准 ID、xAI event/prompt ID、最后按 `(turn ordinal, kind, segment ordinal)` 对账。不得只按文本内容匹配。
- 无法可靠映射的 replay item 不进入可见 timeline：保留对应本地 turn，原始 replay 只进诊断层并记录 `replay_reconciliation_skipped` notice。不得同时保留新旧可见副本，也不能错误合并两个合法相同文本。

## 6. 同进程 follow-up

UI follow-up 满足以下条件才发送：

- task/session 当前无运行 prompt；
- 原 Grok 子进程仍健康，或可按第 7 节恢复；
- 文本非空；
- write/read 模式沿用该 session 的创建模式，不允许 follow-up 悄悄升级权限。

发送后创建新 turn 和用户消息，复用相同 `sessionId` 调用 `session/prompt`。任务 title 可以保持首轮标题，历史中记录每个 turn。

## 7. `session/load` 与恢复

ACP v1 规定历史 notification 在 `session/load` response 之前到达。实现必须：

1. 先在 router 注册 session 与 staging reducer。
2. 再发送 `session/load`。
3. load 期间所有 replay update 进入 staging，不直接追加到当前可见 timeline。
4. load 成功后，在单个 storage transaction 中用 staging 的规范化历史核对/替换可靠映射的可重放 turn；可靠映射的 item 复用既有稳定 itemId，从而保留用户展开状态。commit 后以 generation reset 通知 GUI 原子 resnapshot，不能逐项闪烁。
5. load 失败时保留本地历史，session 标记 unavailable，禁止发送 follow-up，并显示错误。
6. 同一 session 的并发 load 请求合并为一个共享 future。

标准 `messageId/toolCallId` 优先去重；本机 Grok 的 `_meta.isReplay`、eventId、promptId、chunkId 可辅助。不能只按纯文本去重，因为相同文本可能在不同 turn 合法出现。

如果未来 agent 只支持 `session/resume`：恢复上下文但不重放历史，本地 timeline 保持可见，并明确标记它来自本地记录。只有能力存在时才调用 resume。

## 8. 取消

Turn cancel 先以 conditional `(taskId, turnId)` 校验目标并持久化 `terminationCause`，再按精确 lifecycle 分支：

1. `queued`：TaskManager 原子从 FIFO 移除，直接固化 cancelled/failed RunResult；没有 ACP process，不发送 `session/cancel`。
2. `starting` 且 `prompt_dispatched_at` 为空：触发 startup cancel token，中止 discovery/initialize/session-new；若 supervisor 已存在则 TERM/KILL 并确认退出，随后固化结果。不得等待不存在的 prompt response。
3. `running` 或 `prompt_dispatched_at` 非空：把 task 置 cancelling，发送 ACP `session/cancel`；所有待处理 permission request 回复 `{ outcome: "cancelled" }`；等待 prompt 返回 cancelled，软超时 3 秒，再 TERM、等待 2 秒、KILL。未完成 reasoning/tool 标 cancelled并 flush。
4. 每个分支最后都按 terminationCause precedence 写 cancelled 或 failed。若应有的 process tree 在 KILL 后仍无法确认退出，强制 `cancel_timeout`。

Task actor 串行决定 `session/prompt write_all` 与 cancel 的先后；只有 write_all 成功并持久化 `prompt_dispatched_at` 后状态才进入 running，因此没有“可能已发但仍走 pre-prompt cleanup”的模糊窗。

Recovery cancel 使用 `{taskId,recoveryId}`，不借用 last turnId：设置 recovery cancel token，中止 load/router staging，必要时终止本次恢复 supervisor，固化 recovery row 为 cancelled/failed，并让 task 回 interrupted（session 可重试）或 failed（session 不可用）。取消 `auto_resume` 还要持久化 `recovery_state=manual_required`，scheduler 不得立即再次自动 load，直到用户显式 resume/send/retry。它不发送针对旧 prompt 的 `session/cancel`，也绝不改写 last Turn RunResult。

`cancel` 对同一 target ID 幂等。目标 turn 已终态时返回该不可变 RunResult 和当前独立 taskStatus；目标 recovery 已终态时返回其 recovery result。旧 ID 绝不能取消另一轮/恢复。cancel handler 等待相应有界流程完成后返回，除非该 handler 自身被 MCP request cancellation 提前结束。

阻塞 MCP `run` 的客户端断开后，daemon 在 1 秒 grace 后取消其 request 绑定的 client-owned turn；异步 `start` 的 turn 为 daemon-owned，调用方断开不取消。

MCP `notifications/cancelled` 针对某次 `run` request 时，即使 stdio 仍连接，也必须触发同一 client-owned `(taskId, turnId)` cancel。取消 `wait/status` 只停止该等待/读取，不取消 daemon-owned turn；`start` 在 accepted response 前被取消可撤销 submission，accepted 后 turn 仍 daemon-owned；取消 `cancel` handler 不撤销已经发出的 conditional turn cancellation。

## 9. 模型与版本

实际模型显示优先级：

1. ACP session/config update 明确报告的当前模型；
2. xAI session metadata；
3. 本次显式 `model` 参数；
4. 启动前 `grok models` 标记的默认模型；
5. 无法验证时显示 `Grok default`，不得硬编码 `grok-4.5`。

任务保存 Grok CLI version、ACP protocol version、requested model、resolved model 和 reasoning effort。模型列表发现结果可以缓存 10 分钟，失败不阻止继承默认模型的任务。

## 10. 限制与防护

- JSON-RPC 单行默认上限 8 MiB；超限任务失败，不无界分配内存。
- 单个文本 item 默认保留 2 MiB；更大内容写 bounded diagnostic 并在 UI 提示截断。
- terminal output 正常视图默认保留头尾各 200 行，完整 bounded 内容可在诊断中查看。
- terminal/tool output 以 terminalId 为首选关联键、toolCallId 为回退；output 早于 metadata 时进入有界 pending buffer，metadata 到达后原位挂接，exit 早到也保留到对应 terminal。每 terminal canonical buffer 上限 2 MiB/20,000 行，超限保留头尾并写显式截断标记；ANSI parser 维护跨 chunk 状态，持久化净化后的文本与必要 style runs，不持久化可执行转义。
- stderr 使用独立有界 ring buffer，避免子进程因 pipe 填满而死锁。
- 子进程异常退出必须携带 exit code、最后 stderr 摘要和当前动作，不能表现为页面永久 running。
- 每 task 的 raw diagnostic 预算为 64 MiB 或 100,000 events（先到者）；超限时写一次 `diagnostic_budget_exceeded` marker，之后只保留计数、错误与 lifecycle 摘要，不再保存 token 级 raw payload。规范化 timeline 另有 256 MiB/task 预算，超限后工具/终端/思考按头尾截断，但继续保留状态、最终 answer 与明确 marker，不能导致任务死锁。
