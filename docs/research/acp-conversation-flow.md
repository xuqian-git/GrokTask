# GrokTask ACP 对话流开源实现调研

状态：研究记录；最终决策以 `docs/specs/` 为准。用户已确认 active Plan 始终完整展开。

## 目标

解决旧 Activity 页面中的四类问题：

1. ACP 通知和事件以原始 JSON 或内部名称进入主界面，用户看不出当前在做什么。
2. 固定的“思考 / 工具 / 回复”分组破坏真实到达顺序，无法表达 Thought → Tool → Thought → Reply。
3. 流式内容增长时滚动行为与用户操作冲突。
4. 状态更新会重建卡片或覆盖用户手动展开状态。

## 调研基线

| 实现 | 固定版本 | 借鉴重点 | 明确不照搬 |
| --- | --- | --- | --- |
| Zed | `afc13dc8e054705a42e2931b0840ed03ccfa583a` | 有序语义时间线、Thought 与 Reply 交错、Tail 滚动状态机、load 竞态处理、活动 Plan 条 | GPL 代码不复制；工具完成后主动折叠不采用 |
| Harnss | `dc1dfd8a33caa46a1eefcfe9e14697b27ac4c33d` | 阶段级思考、语义工具卡、手动展开保护、bottom-lock、流式缓冲 | 不整体移植 Electron 应用；不采用两秒后自动折叠 |
| ACP Components | `1b5ab3a1dbdcd194e2550b710d652d9d9670f48c` | 有序 `parts`、约 16ms 批处理、边界前 flush、虚拟列表 | 思考完成后强制折叠不采用 |
| Obsidian Agent Client | `89e2d75c4e35e78aaaa21b8a6e7ca34e4e0ce099` | 轻量、严格时序的 Thought / Tool / Reply；接近底部才跟随 | 不采用只适合 Obsidian 的宿主集成 |
| Nori CLI | `3fce409322c643e6d62aabfac55fb26da7c55843` | 事件语义化、简洁动作标题、版本化完整 transcript | 主视图不隐藏用户希望看到的阶段思考 |
| Jockey | `c7431a81469c77d0cc821dc39c0c8905399a22fa` | 工具与文本交错、终端输出缓冲、内联权限 | 不采用只恢复最终文本的历史模型 |
| Agentic.nvim | `a37f19f1d7eda70267c73c65dd7c1c1d4f47c971` | 稳定折叠状态、工具旁权限请求 | 不照搬 Neovim 专用 UI |
| ACP UI | `cd9c3cb464a4b321bff652101953a64c07473e31` | 最小 ACP 客户端基线 | 固定分区和无条件滚到底 |
| VSCode ACP | `e7371659e3ac100db842b419b1361205a193032e` | load replay 的基本处理 | 无条件滚动、自动折叠覆盖用户选择 |
| acpx | `a518ea909eb91296b0d05c76345f1c8403ba830b` | prompt drain、恢复、事件模型与测试 | alpha 工具，不把它当规范来源 |

规范基线为 Agent Client Protocol 仓库提交 `2642126f4e0e4dbab61740039bf0d3049a2af9e4`，Grok 本机实测版本为 `0.2.101`。

## 推荐的信息架构

主视图不再显示“ACP 通知”“事件”或原始 JSON，而是显示严格按发生顺序排列的语义时间线：

```text
用户消息
  思考阶段 1（流式、可展开）
  读取 src/server.ts
  思考阶段 2（流式、可展开）
  修改 src/store.ts（+18 −6）
  运行 pnpm test
  思考阶段 3
  最终回复（Markdown）
```

标准时间线项：

- `user_message`
- `reasoning_segment`
- `assistant_segment`
- `tool_call`
- `plan_snapshot`
- `permission_request`
- `context_notice`

终端输出是 `tool_call` 的详情，不在主时间线中再生成一类噪声事件。ACP 未识别扩展通知只写入诊断日志。

## 归一化规则

### 文本与思考

- 相同类型的连续 chunk 合并成一个 segment。
- Thought、Tool、Plan 或 Reply 互相切换时先 flush 当前 segment，再插入下一项，保留真实时序。
- Grok 当前不提供标准 `messageId`；运行时按 turn、流类型和边界生成稳定本地 ID，并持久化。
- 约每 16ms 批量刷新一次 UI；按 Unicode 字符边界增量揭示，避免中文、emoji 断裂和 Markdown 闪烁。
- 最终回复不是额外复制的一张卡，而是最后一段公开 assistant Markdown。

### 工具调用

- `tool_call_update` 按 `toolCallId` 原位合并，不追加通知卡片。
- 主行只回答“这一步在做什么”。标题优先级：ACP `title` → 内容首个文本 → 单一路径 → 人类化工具类型与名称。
- 折叠态示例：`读取 src/server.ts`、`搜索 session/load，找到 7 处`、`运行 pnpm test`。
- 展开态按工具类型展示命令、路径、精简输出、diff、错误或权限；原始 JSON 只在独立诊断页可见。
- 读取、搜索等连续且已完成的轻量动作可以聚合；编辑、终端、错误和权限请求不得被聚合隐藏。

### Plan

- `plan` 是当前完整快照，后续通知替换同一个逻辑 Plan，不追加重复卡片。
- 借鉴 Zed 的位置：实时 Plan 作为时间线与输入框之间的活动状态条。最终产品决策不是折叠态，而是始终显示所有步骤；header 仍显示当前步骤与完成计数。
- 计划完成后，向时间线插入一次只读完成快照，保留历史上下文。

### 展开状态

每个思考块和工具卡使用稳定 ID 保存三态：

```text
auto
user-expanded
user-collapsed
```

自动逻辑只允许改变 `auto`。用户一旦手动展开或收起，流式更新、工具完成、任务完成和重渲染都不能覆盖该选择。

### 自动滚动

- 初始处于 `following-tail`，最后一项增长时保持真正的底部可见。
- 只有检测到 wheel、touch 或滚动条拖动等用户意图，并且用户离开底部阈值时，才暂停跟随。
- 内容高度变化、Markdown 重排或虚拟列表测量不能误判成用户滚动。
- 用户回到底部或点击“回到最新”后恢复跟随。
- 暂停跟随时显示“回到最新”按钮，并可显示未读更新数量。

## 菜单栏浮层与完整窗口

两者使用同一个归一化时间线与状态存储，不做两套 ACP 解析。

菜单栏浮层保留：

- 任务标题、运行状态、实际模型；
- 最近的阶段级思考摘要；
- 最近若干语义工具动作；
- 当前 Plan 步骤；
- 继续输入、取消任务、打开完整窗口。

完整窗口展示全部时间线、展开详情、历史任务和诊断入口。菜单栏浮层不承载完整原始 transcript。

## ACP 生命周期约束

本机 Grok `0.2.101` 实测：

- `initialize` 返回 `loadSession: true`，但不提供 `sessionCapabilities.resume`。
- 多轮继续应在同一进程与同一 `sessionId` 上串行调用 `session/prompt`。
- 进程重启后使用 `session/load`；历史 `session/update` 会在 load 响应之前到达，因此必须先注册 reducer 和订阅者，再发送 load。
- load replay 与本地 transcript 必须基于稳定 ID / xAI event ID 去重，不能重复显示。
- `session/prompt` 响应只是 `stopReason`；公开回复来自 `agent_message_chunk`。
- `available_commands_update` 与 `_x.ai/*` 等扩展通知不进入主对话。
- cancel 先发送 `session/cancel`，取消待处理权限并等待有界时间，再执行 TERM/KILL 兜底。

## 建议默认行为

1. 采用“严格有序时间线”，淘汰固定阶段卡片。
2. 每段连续 reasoning 各自形成阶段级思考块；流式收起时显示最近三行预览，完成后显示一句摘要。
3. 默认自动态可在流式时显示预览；用户手动展开后永久保持，不自动折叠。
4. 工具主行只显示语义动作，详情按需展开。
5. 实时 Plan 使用底部完整展开的活动区，完成后在首次出现位置显示一个历史快照。
6. 全窗口保留完整语义 transcript；菜单栏浮层仅保留当前上下文摘要。

## 验收要点

- Thought → Tool → Thought → Reply 的顺序与 ACP 到达顺序一致。
- 相同 `toolCallId` 的所有更新只对应一个工具卡。
- 正常视图不出现 ACP 方法名、通知名或原始 JSON。
- 用户未主动滚动时始终看见最新内容；向上滚动后新 chunk 不改变视口；回到底部后恢复跟随。
- 用户手动展开的思考或工具卡在状态更新和任务结束后仍保持展开。
- Markdown 流式更新不闪烁，不破坏中文、emoji、代码块和列表。
- load 响应前到达的 replay 通知不丢失，不重复；并发 load 合并为一次底层请求。
- 只能恢复上下文而不能重放历史时，UI 明确提示；当前 Grok 路径优先使用已实测可用的 load。

## 主要参考

- Zed ACP thread 与 UI：<https://github.com/zed-industries/zed/tree/afc13dc8e054705a42e2931b0840ed03ccfa583a/crates/acp_thread>
- Harnss：<https://github.com/OpenSource03/harnss/tree/dc1dfd8a33caa46a1eefcfe9e14697b27ac4c33d>
- ACP Components：<https://github.com/zvzuola/acp-components/tree/1b5ab3a1dbdcd194e2550b710d652d9d9670f48c>
- Obsidian Agent Client：<https://github.com/RAIT-09/obsidian-agent-client/tree/89e2d75c4e35e78aaaa21b8a6e7ca34e4e0ce099>
- Nori CLI：<https://github.com/tilework-tech/nori-cli/tree/3fce409322c643e6d62aabfac55fb26da7c55843>
- Jockey：<https://github.com/recailai/jockey/tree/c7431a81469c77d0cc821dc39c0c8905399a22fa>
- Agentic.nvim：<https://github.com/carlos-algms/agentic.nvim/tree/a37f19f1d7eda70267c73c65dd7c1c1d4f47c971>
- ACP 规范：<https://github.com/agentclientprotocol/agent-client-protocol/tree/2642126f4e0e4dbab61740039bf0d3049a2af9e4>
- xAI Grok ACP：<https://docs.x.ai/build/cli/headless-scripting>
