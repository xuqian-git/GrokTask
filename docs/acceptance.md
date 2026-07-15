# GrokTask 验收标准

状态：实现与 review 的最终门槛。

## 1. 必须通过的命令

具体 script 名允许由 Grok 在 Phase 0 定义，但必须提供等价的一键入口：

```text
pnpm install --frozen-lockfile
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features
cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol
```

如某项工具不适合最终栈，README 必须说明等价替代；不能简单删除质量门槛。

## 2. 产品形态

- [ ] 仓库构建单一 `GrokTask` binary。
- [ ] `mcp`、daemon、CLI 角色不初始化 Tauri/AppKit/WebView。
- [ ] GUI host 单实例，daemon 单实例。
- [ ] 仓库不再包含 Codex plugin manifest、skills、hooks、Node companion 或 localhost dashboard runtime。
- [ ] 运行时不监听 TCP/HTTP 端口，UI 不要求 Chrome 或 Codex 内置浏览器。

## 3. MCP 与 CLI

- [ ] MCP 只暴露 `run/start/status/wait/cancel` 五个工具。
- [ ] `run/start` 缺少 mode、cwd 或 task，或 `start` 缺少合法 submissionId 时在 spawn 前失败。
- [ ] `mode` 只接受显式 `read|write`，没有默认和文本推断。
- [ ] `run` 阻塞返回 Markdown、taskId、turnId、实际模型、stopReason 与时间；partial 的 text fallback 明确标成部分结果。
- [ ] `start` 返回 taskId+turnId 后任务继续；`status/wait/cancel` 可跨 MCP 进程使用，wait(T1) 在 UI 已启动 T2 后仍稳定返回 T1。
- [ ] `start` 的 caller-generated submissionId 在 commit 后丢 response、重连并 retry 时返回同一 taskId/turnId；同 ID 不同输入冲突，绝不创建第二个 task。
- [ ] wait 超时返回非终态快照，不误报 task failure。
- [ ] cancel 幂等。
- [ ] cancel(T1) 在 T1 已终态且 T2 running 时返回 T1 的不可变 RunResult 与独立 taskStatus=running，不伪装 idle、不影响 T2。
- [ ] queued Turn cancel 直接 dequeue；pre-prompt starting cancel 中止 startup/process；只有 prompt_dispatched/running 才发送 ACP session/cancel，各路径都在有界时间固化结果。
- [ ] MCP/CLI stdout 不含 daemon log、Grok stderr 或 Tauri log。
- [ ] MCP `notifications/cancelled` 取消单个 run 会传播到 ACP；取消 wait/status 不误取消异步 task。
- [ ] T1 完成后 UI 并发启动 T2，T1 的迟到 request cancellation/conditional cancel 不会终止 T2；turn owner/request binding 可从持久化与运行时状态核对。
- [ ] Codex/Claude 配置写入 24h timeout 和当前 executable 绝对路径。

## 4. Read/write 安全

- [ ] read argv 包含 read-only sandbox、dontAsk、Read/Grep allow、Edit/WebFetch deny、禁用 web search/subagent，且不包含宽泛 `Bash(git *)`。
- [ ] write argv 包含 workspace sandbox、明确非交互批准模式，以及常见直接 commit/push/PR/clean/reset-hard/rm-rf deny；文档不把 glob 误称为不可绕过边界。
- [ ] fixture 工作区 read E2E 前后 hash 一致。
- [ ] write 的项目写入被 sandbox 限制在 cwd；只允许 Grok runtime 使用 `/tmp` 与 `~/.grok` 例外，不能写其它用户目录；GrokTask wrapper 自身不提交、推送或创建 PR。
- [ ] daemon 崩溃后不自动继续未完成 write prompt。
- [ ] daemon 崩溃后 read/write 都不自动重发未完成 prompt；恢复只重建历史与可继续状态。

## 5. ACP 正确性

- [ ] initialize 后按 capability 决定 load/resume，omitted capability 视为不支持。
- [ ] 同一 session prompt 串行，不发生两个并发 `session/prompt`。
- [ ] prompt response 只作为 stop metadata；answer 来自 assistant chunks。
- [ ] Thought → Tool → Thought → Reply 顺序与 fixture 完全一致。
- [ ] 连续 token thought 合并成阶段，不生成逐 token 卡。
- [ ] tool update 按 toolCallId 原位合并；update-before-create 仍只有一张卡。
- [ ] plan notification 是 full replacement；completed step 后与 drain 内的 late plan 仍更新同一 anchor，turn drain 后才冻结一个历史快照。
- [ ] prompt response 后迟到 chunk 在 drain 窗内进入同一 turn。
- [ ] end_turn/max_tokens/max_turn_requests/refusal/cancelled/unknown stopReason 都按规格映射，partial/refusal 不伪装成正常 final。
- [ ] 本地 terminationCause 优先于 ACP cancelled：用户/MCP cancel→cancelled，read violation/permission unavailable/hard timeout/cancel timeout 保持各自 failed error，不被通用 cancelled 覆盖。
- [ ] unknown/custom xAI notification 不 panic、不进入主对话。
- [ ] cancel 先合作式，超时后终止完整 process tree。
- [ ] daemon 被强制 kill 后 supervisor 在有界时间内清理 Grok process tree；新 daemon 不误杀 PID reuse 进程。
- [ ] read/write/cancelling 三种 permission request 都在 2 秒内得到 selected/cancelled response，任务不悬挂。
- [ ] permission 强制失败在响应 request 后继续走 session cancel 并确认 prompt 停止，不向 failed turn 继续写可见内容。
- [ ] permission 在到达位置显示人类可读的 requesting→allowed/rejected/cancelled 语义行并同步 tool substatus，不显示 ACP JSON。
- [ ] output-before-terminal、exit-before-metadata 和超限 terminal fixture 正确关联、截断且不破坏 ANSI chunk 状态。

## 6. Session 恢复

- [ ] router/staging reducer 在发送 load 前注册。
- [ ] load response 前到达的全部 replay notification 不丢失。
- [ ] replay 不与本地历史重复；相同文本在不同 turn 不被误删。
- [ ] 无法可靠对账的 replay 保留本地可见 turn、丢弃不确定 replay 的可见副本，并记录 notice。
- [ ] 同一 session 并发 load 只有一个底层请求。
- [ ] load 成功后 follow-up 复用原 sessionId。
- [ ] load 失败保留本地历史并禁用 follow-up，错误可行动。
- [ ] cancelling 状态崩溃恢复后只完成取消，不执行 load；queued owner 与 interrupted/recovering 公开状态按规格映射。
- [ ] daemon crash 把旧活动 Turn 固化为 failed/daemon_interrupted，使 wait(T1) 立即有结果；只有 Task container 进入 interrupted，恢复不会改写 T1。
- [ ] crash 时尚无持久化 sessionId 的 task 直接 failed/session_unavailable，不自动猜测或重发 prompt。
- [ ] interrupted read 自动 load 但不重发旧 prompt；interrupted write 保持等待显式 resume/send/retry。`task.continue` 的 expectedLastTurnId 防双发，每次 retry 创建新 turnId。
- [ ] recovering 使用 recoveryId 取消/force restart/crash，last failed Turn result 永不改写；Task 回 interrupted/failed，status 暴露 activeRecoveryId；取消 auto_resume 后 manual_required 防止立刻重启恢复。

## 7. 对话 UI

- [ ] 正常视图 DOM 不出现 `session/update`、`tool_call_update`、`_x.ai` 或完整 JSON payload。
- [ ] 每个连续 reasoning stage 在发生位置展示；流式三行 preview，完成一句摘要。
- [ ] 用户手动展开 thought/tool/长用户消息/聚合行/历史 Plan 后，完成、成员增长、刷新、窗口切换、历史回放均不自动折叠。
- [ ] user-expanded 的 read/search 不会因后续轻量动作聚合而消失。
- [ ] assistant 最终回复使用安全 Markdown，不复制第二份 final card。
- [ ] tool 主行可读地说明“正在做什么”，错误/编辑/终端不被聚合隐藏。
- [ ] active Plan 固定在 timeline 与 composer 之间并始终完整展开；snapshot 原位更新，`plan_finalize` 用一个前端 store commit 同时隐藏 bar、在 originSequence 显示一次历史记录。
- [ ] popover 与完整窗口使用相同 item ID 与展开状态。
- [ ] 无标准 messageId 的 segment 在 live、重启和 load replay 后保持相同 namespaced item ID。
- [ ] 两个 WebView 的展开修改通过 IPC revision 实时同步；scroll/follow/draft 各 surface 独立。
- [ ] 权限语义行解释自动允许/拒绝/取消，不会让两个 reasoning block 无解释地相邻。
- [ ] 10,000 个连续轻量动作中，聚合组最多 100 项；user-expanded 成员作为外层平坦 virtual rows，不挂载巨型嵌套 DOM。
- [ ] 100+ step active Plan 在 popover/完整窗口内可滚动且窗口化，timeline/composer 保有最低空间。

## 8. 滚动

- [ ] 用户从未主动离开底部时，新 chunk 和高度变化持续跟随最新内容。
- [ ] 用户 wheel/touch/拖动上滚后，新内容不改变视口。
- [ ] 程序化 resize/Markdown reflow 不会错误重新锁到底部。
- [ ] 用户回到底部或点“回到最新”后恢复跟随。
- [ ] detached 时显示未读/回到底部控件。
- [ ] 展开旧卡片不会强制跳到末尾。
- [ ] detached 工具项在聚合形成/prepend/merge/split 后通过 underlying→render projection 保持屏幕位置，不因被折进聚合行而跳动。
- [ ] 内层 thought/terminal 滚动不会解除主 timeline bottom-lock。
- [ ] 同一 popover/task 隐藏再打开按 `{anchorItemId,intraItemOffset,lastSeenSequence}` 保留 detached anchor 和未读数；Markdown reflow、聚合拆分、旧位置 Plan 插入后仍稳定，首次打开新任务才默认跟随底部。

## 9. Markdown 与内容安全

- [ ] raw HTML/script/event handlers/javascript URL 不执行。
- [ ] 外部链接交给系统浏览器，不在 WebView 内导航。
- [ ] 远程图片默认不自动加载。
- [ ] 中文、emoji、组合字符和 fenced code 跨 chunk 仍正确。
- [ ] terminal ANSI 安全解析，超长输出明确截断。

## 10. 持久化

- [ ] DB transaction commit 后才广播 sequence。
- [ ] snapshot@B/delta-to-B response 必定先于 `>B` live event；独立 producer 加 per-subscriber backlog 时同时到达的 mutation 不丢失、不重复。
- [ ] 256 MiB/10,000-item 大 snapshot 遇到慢 subscriber 与持续 ACP chunks 时，Task actor 继续 drain；仅该 subscriber 超预算/超时断开，不暂停 Grok 或其它窗口。
- [ ] 大 snapshot/delta 以 <=1 MiB chunk 流式传输，单个 >1 MiB item 以带 hash 的 fragments 重组；snapshot_end 前不发 live，中途断线不展示半个 snapshot。
- [ ] GUI sequence gap 或 generation reset 会原子重新 snapshot；load 期间看不到逐项 remove/add 中间态，失败 rollback 保持旧 timeline。
- [ ] delta 只读取同 generation mutation；reset 后 materialized snapshot 是权威，不错误尝试从旧 generation 增量重建。
- [ ] 默认保留 200 task，设置可改，活动任务不被 retention 删除。
- [ ] timeline snapshot 与 ui_state rows 在同一 B/U barrier；持久化 uiState generation/revision 跨 daemon restart 单调，两个 WebView 不漏 change、不把新 generation 当旧事件。
- [ ] idle Grok child 按 30 分钟/3 个 LRU 回收，cold task follow-up 可 lazy load；historyLimit=0 使用持久化 30 分钟 retentionProtectUntil，daemon 5 分钟退出也不提前删除。
- [ ] 清空历史级联删除 timeline/raw/ui state；idle-warm task 先停 supervisor，可见/有 request lease task 默认 skipped；并发 follow-up/wait/snapshot/ui_state.set 在 deletion guard 前持 lease使 clear skip，或 guard 后被拒绝，不产生 FK 写入/race。
- [ ] expansion 用户态跨重启保存。
- [ ] popover/full window 并发切换展开状态按 server revision last-write-wins，同步且无旧回声覆盖。
- [ ] A→B→A 快速切换及两个 WebView 交错 snapshot 按 selection/subscription epoch + client streamId 隔离；同 task S1→S2 的迟到 S1 end 不覆盖 S2，timeline/UI-state 缓存各自单调前进。header 前 unsubscribe/关闭 data connection 也立即释放 producer/read txn/backlog/lease。
- [ ] B snapshot 缓慢/失败时，committed A 仍持续应用 live；B 成功续租后才原子 promote，A→B→C 只清 pending。
- [ ] timeline 不变而另一 WebView 只修改 disclosure 时，迟到旧 snapshot 不覆盖较新的 uiStateGeneration/revision。
- [ ] 数据库 migration 失败不删除或重建用户数据。
- [ ] raw diagnostics 与日志做 credential redaction。
- [ ] raw/normalized/global budget 到达后产生可见 marker、不中断 ACP drain，并按规则只裁剪 diagnostics/重型详情。

## 11. 托盘与窗口

- [ ] macOS 菜单栏、Windows/Linux tray 左键打开锚定 popover，右键原生菜单。
- [ ] popover clamp 在 monitor work area 内，Linux 缺坐标时有右上角回退。
- [ ] popover 可输入 follow-up，点击外部隐藏但不取消任务。
- [ ] popover 隐藏保留未发送草稿；重新打开恢复输入焦点，隐藏后不抢占此前应用焦点。
- [ ] popover auto/user-pinned 选择规则确定；pinned/detached 不被后台新任务抢占，auto 按状态、updatedAt、taskId 稳定排序。
- [ ] `off|active|always` 行为与规格一致；只有 always 安装登录项。
- [ ] 完整窗口/设置/历史全局单实例，重复打开只导航和聚焦。
- [ ] 无 GUI 或 Linux 无 tray 时 MCP/CLI/daemon 仍可工作且 doctor 清楚降级。
- [ ] Linux tray 无左键 activate 时菜单仍能打开当前任务，doctor 报告能力降级。
- [ ] off/active/always 在空闲 MCP 长连接与可见窗口 lease 下按规格起停；lease acquire/renew/release、断线回收、首次 subscribe 原子 task lease 均通过，always host 不保活 daemon。
- [ ] A→B task switch 先 acquire B、B commit 后才 unsubscribe/release A；失败/快速切换/隐藏路径无 stale task lease 阻止 retention 或 daemon idle。
- [ ] 同 task S1→S2 也使用不同 lease；pending 每 30 秒 renew、45 秒 deadline，commit 前立即续满 60 秒。成功/失败及 44–45 秒边界均保留正确 committed lease。
- [ ] 完整窗口加载最大历史时使用独立 stream connection，popover live/control latency 有界，不被 snapshot frames head-of-line 阻塞。
- [ ] 自动 binary replacement 对 queued/starting/running/cancelling Turn、active recovery、accepted start/continue 与 in-flight run/wait 都 bounded drain，10 分钟 retryUntil/deferred 契约一致且不取消任务；显式 restart 无 force 拒绝、force 才按目标类型取消；handoff 崩溃可恢复。
- [ ] 正常 handoff/idle exit 在关 child 前把本实例 idle+warm 原子改 cold并清 supervisor identity；crash 后新 daemon 修复 stale warm，绝不 attach 或计入 LRU。
- [ ] accepted continue 在状态生效与 response 安全交付之间触发 replacement 时，结果不丢且重试不会生成第二个 turn；start 同样受 delivery barrier 保护。

## 12. Agent 配置编辑

- [ ] Codex TOML 只修改 `[mcp_servers.groktask]`，保留其它内容与注释。
- [ ] Claude JSON 只修改 `mcpServers.groktask`，保留大文件其它内容。
- [ ] invalid config 中止且原文件逐字节不变。
- [ ] install/update 幂等；remove 不触碰其它 server。
- [ ] binary path 或 timeout 变化可检测为 outdated。

## 13. 真实 Grok 冒烟测试

在本机已登录 Grok 时运行 opt-in E2E：

1. read 任务回复 `hello`，UI 与 MCP 都收到非 JSON 的流式/最终文本。
2. read 读取/摘要一个 fixture，确认工作区未变；review、bug 排查和性能分析由调用方完成。
3. write 修改 fixture 文本，确认变更发生且 MCP 返回最终 Markdown。
4. 至少一个包含 thought + tool + reply 的任务，人工对比终端 Grok 与 GrokTask 顺序。
5. 同 session 从 popover 发送 follow-up，确认上下文连续。
6. 运行中取消，确认子进程树退出、UI 终态和 MCP 结果一致。
7. 结束 daemon 后重启并 load 历史，确认不缺段、不重复。

真实 E2E 不能作为普通 CI 强制项，但最终交付前必须在当前 macOS 环境执行并记录结果。

## 14. 跨平台 CI

- [ ] macOS arm64/x86_64、Windows x86_64、Linux x86_64 compile/build job 通过。
- [ ] Windows named pipe 与 process-group 代码不是 `todo!()`/空 stub。
- [ ] Linux tray unavailable 路径有测试。
- [ ] release artifact 带 version 与 SHA-256 checksum。
- [ ] 10,000 timeline items 持续更新时使用窗口化 DOM，detached anchor 稳定，popover 不发生秒级卡顿。

## 15. 完成定义

只有同时满足以下条件才算完成：

- 所有自动门槛通过；
- 当前 macOS 真实 Grok 冒烟与托盘交互通过；
- Codex 最终 code review 没有 P0/P1 finding；
- P2 要么修复，要么在 AskHuman 中得到用户明确接受；
- README 与实际命令一致；
- 用户通过 AskHuman 明确确认可以结束任务。
