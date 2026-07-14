# GrokTask 对话流与桌面 UI 规格

状态：已由用户确认。研究依据见 `docs/research/acp-conversation-flow.md`。

## 1. 核心原则

1. 顺序优先：按 ACP 到达顺序展示，不把一轮强行重排成固定“思考、工具、回复”三段。
2. 语义优先：主行说明 Grok 正在做什么，不显示协议名或 JSON。
3. 原位更新：streaming text、tool status 和 plan update 更新已有 item，不制造重复行。
4. 用户状态优先：自动逻辑不能覆盖用户手动滚动与展开选择。
5. 同源：popover、完整窗口、历史回放使用同一 timeline snapshot 与 reducer。

## 2. 完整窗口布局

```text
+----------------------+------------------------------------------------+
| 最近任务 / 搜索       | 标题  [READ/WRITE] [模型] [状态]       ...     |
|                      |------------------------------------------------|
| 今天                  | 用户提示                                       |
|  ● 重构 ACP UI        |                                                |
|  ✓ Review API         | 思考阶段：正在检查事件顺序                     |
|                      |   三行预览或完整 Markdown                       |
| 昨天                  |                                                |
|  ! 修复构建           | 读取 src/server.ts                             |
|                      | 思考阶段：正在设计 reducer                      |
|                      | 修改 src/store.ts                    +18 -6    |
|                      | 最终 Markdown 回复                              |
|                      |                                                |
|                      |------------------------------------------------|
|                      | 当前步骤 · 2 项剩余                         v  |
|                      | [继续让 Grok...]                         [发送] |
+----------------------+------------------------------------------------+
```

- 默认窗口最小 900×640，初始 1120×760；保存用户最后尺寸与位置。
- 左侧任务栏默认 280px，可折叠；popover 不显示侧栏。
- Header 显示任务名、read/write、实际模型、运行状态、开始时间和更多菜单。
- Timeline 是唯一主滚动容器；Plan bar 与 composer 固定在底部，不覆盖最后一条内容。
- 历史任务切换先显示 SQLite snapshot，再订阅该 task 的 sequence 增量。
- 每个 surface 显式维护 `committedSelection` 与至多一个 `pendingSelection`，两者各含 taskId/selectionEpoch/subscriptionEpoch/streamId/leaseId。点击 B 时 A 仍 committed、继续应用 A live；B 只写 staging。B snapshot 校验并续租成功后才原子 promote 为 committed，再 unsubscribe/release A；B 失败不动 A，A→B→C 只清理 pending B。每次选择意图递增 `selectionEpoch`；每次 subscribe（包括同 task 的 gap/reset/reconnect）另递增 `subscriptionEpoch`。每个 frame 必须匹配 committed 或 pending 的完整 tuple；pending 只有 end 能触发 promote。同 task 旧 stream 也不能覆盖新 stream。迟到 stream 对后台 timeline cache 与 UI-state cache 分别做单调 CAS，绝不能降级任一版本域或导航界面。

## 3. Popover 布局

- 默认 420×620，允许设置 360–520px 宽、420–760px 高。
- 顶部：当前任务标题、状态、READ/WRITE、实际模型；多任务时可切换活动任务。
- 中部：同一 timeline 的紧凑渲染，保留阶段思考与语义工具动作；重型 diff/terminal 详情提示“在完整窗口查看”。
- 底部：活动 Plan bar、follow-up 输入、取消/打开完整窗口。
- popover 隐藏后保留 selection mode、选中任务、输入草稿、滚动/follow、未读与展开状态；再次打开同一任务不重置。隐藏同时 unsubscribe committed/pending stream 并 release 对应 task lease，状态本身留在 GUI host 内存。
- 无任务时显示最近完成任务和“打开历史/设置”，不显示空白日志框。

## 4. 时间线 item

凡是提供展开/收起的 disclosure，都使用同一三态 `auto | user-expanded | user-collapsed`，并通过稳定 `disclosureKey` 持久化。普通 item 使用 `item:<itemId>:<part>`；长用户消息的 part 为 `body`，thought/tool/历史 Plan 为 `details`。自动完成、snapshot 更新、聚合成员增长、窗口切换和历史回放都不得覆盖用户态。Active Plan 按已确认设计始终展示完整步骤，不提供 disclosure。

### 4.1 用户消息

- 使用清晰但克制的背景，与 agent 内容区分。
- 长文本可折叠；使用 `item:<itemId>:body` 保存三态，完整内容始终可访问。
- 显示发送时间与 turn 序号，不显示 ACP request ID。

### 4.2 阶段级思考块

每个连续 `reasoning_segment` 是独立思考块，出现在发生位置。任何不同可见类型——工具、Plan、权限、context notice 或公开回复——都会先结束当前块；之后的 thought 创建下一块。

折叠态：

- 流式时标题为“正在思考”，附轻量 shimmer；下方显示最近三行动态 Markdown 预览。
- 完成后标题改为阶段摘要。摘要优先使用 xAI 明确提供的阶段标题；否则取首个 Markdown 标题；再否则取第一个非空完整句；仍没有完整句时取去除 Markdown 后的首个非空 80-grapheme 片段，最终 fallback 为“思考过程”。标题最多 80 个 grapheme。
- 摘要不能调用第二个模型生成，也不能显示“ACP notification”。

展开态：

- 显示完整安全 Markdown，最大高度 320px，内部可滚动；完整窗口可选择“展开到内容高度”。
- 流式时只有内部滚动仍在底部才跟随；用户在块内上滚后暂停。
- 用户手动展开后，segment 完成、任务完成、数据刷新和历史回放都不能自动折叠。

展开状态：

```text
auto | user-expanded | user-collapsed
```

默认 `auto`：流式显示三行 preview，完成后显示一句摘要。`user-expanded` 始终显示完整内容；`user-collapsed` 只显示标题/摘要，不再自动展开三行 preview。任何手动点击转换为用户态，自动逻辑不得再修改。

### 4.3 工具卡

折叠主行由图标、动词、目标、状态与可选统计组成：

```text
◌ 正在读取  src/server.ts
✓ 搜索了    “session/load” · 7 处
✓ 修改了    src/store.ts                  +18 −6
✕ 运行测试  pnpm test · 2 个用例失败
```

- pending/running 使用现在时，completed 使用过去时，failed 使用明确失败文案。
- status update 原位改变同一行，不能新增“tool update”事件。
- 工具 title 已足够时优先使用 ACP title；路径和命令只补充必要上下文。

展开详情按类型渲染：

- read/search：路径、query、命中摘要；
- edit/write：统一 diff、文件统计；
- terminal：command、cwd、ANSI 输出、exit code，默认限高；
- web：URL 与摘要；
- error：错误摘要与可复制详情；
- unknown：只展示安全的文本内容，原始 JSON 在诊断页。

手动展开状态与 thought 相同，itemId 采用 namespaced `tool:<sessionId>:<toolCallId>`，disclosureKey 为 `item:<itemId>:details`，永不自动折叠。

### 4.4 权限语义行

- permission request 在到达位置显示一条紧凑语义行，例如“Grok 请求运行命令权限 · 已按 READ 模式拒绝”或“已按 WRITE 模式单次允许”；不显示 ACP、request ID、option 或完整 payload。
- 生命周期原位更新为 `requesting -> allowed_once | rejected | cancelled`，同时更新关联工具卡的 permission substatus。`requesting` 即使持续很短也保留顺序边界，完成态可视觉弱化但不能消失。
- 权限行永不参与轻量聚合。异常拒绝/不可用结果必须可读地解释 turn 为何停止。

### 4.5 轻量动作聚合

只有同时满足以下条件才聚合：

- 相邻；
- 都已 completed；
- 类型为 read、search 或 explore；
- 中间没有 reasoning、assistant、plan、error、edit、terminal 或 permission item。
- 所有成员都保持 `auto` 或 `user-collapsed`；任一成员为 `user-expanded` 时禁止聚合，确保新动作不会视觉上吞掉用户已展开的卡片。

聚合行例如“探索了 8 个文件”，展开后仍按原顺序显示每个动作。聚合是纯渲染行为，不能改动持久化 timeline 或 item ID。每个组最多 100 个成员，超出按原顺序切成下一组；展开时不是在一个巨型 row 内挂载全部 children，而是把 header/member 投影成同一外层虚拟列表的平坦 rows。自动组初始 key 为 `aggregate:<firstMemberItemId>`；一旦该 key 有 user-expanded/user-collapsed row，它就是 protected group anchor：后续 eligible 尾项可加入且 key 不变，较早完成的前置项不得 prepend，另一个 protected group 也不得与它合并。两个 auto group 可自由合并（仍受 100 项 cap）；前方 protected group可吸收后方 auto group，后方 protected group 绝不吸收前方项；两个 protected group 保持分开。这样每个 WebView 可仅凭 timeline + ui_state 确定性重建同一分组，不需要易竞态的持久化 render row。成员自身被 user-expanded 导致分组拆开时，保留仍存在的 protected anchor 与各稳定成员 itemId。

### 4.6 Assistant 回复

- 每个 `assistant_segment` 在真实位置流式显示；任何 Tool、Plan、permission、context notice 或 reasoning 边界后继续的文本都是新的 segment。
- 使用 GitHub-flavored Markdown 子集：段落、标题、列表、引用、链接、表格、任务列表、行内代码和 fenced code。
- 流式过程中保留纯 canonical text；每帧最多重渲染一次。
- turn 结束时按结果标记最后一段：仅 end_turn 是最终回复，limit 是部分回复，refusal/cancelled/failed 不标 final；不复制到另一张“最终答案”卡。
- MCP 返回的 `answer` 是该 turn 所有公开 assistant segments 按顺序拼接后的 Markdown；UI 仍保持它们在时间线中的原位置。

### 4.7 Context notice

只有用户可采取行动或会影响结果理解的状态才进入时间线，例如：

- Grok 进程意外退出；
- session 只恢复了上下文而没有 replay；
- 输出被截断；
- write 任务在 daemon 崩溃后没有自动继续；
- Grok CLI 未登录或版本不兼容。

普通 lifecycle、usage 和 command list 不显示为 notice。

## 5. Plan bar

- 位于 timeline 与 composer 之间，始终只有一个 active plan。
- Active Plan 始终按原顺序提供所有 step 的 pending/running/completed 状态与 priority；“完整”表示无摘要隐藏、每步都可访问，不要求同时撑开布局。Header 显示当前 running/第一个 pending 与 `已完成/总数`。完整窗口 max-height 为 `min(320px, 35vh)`，popover 为可用内容高度的 35% 且至少给 timeline 160px、composer 96px；内部 `overscroll-behavior: contain`。超过 20 step 使用窗口化列表，100+ step 也不能挂载全部 DOM。首发不把 active Plan 折成单行摘要。
- 首次 `plan` 在 reducer 中建立带 originSequence 的隐藏 anchor；每次 snapshot 原位替换，不追加新卡。
- turn drain 完成时，一个原子 `plan_finalize` projection 同时隐藏 active bar，并把同一 itemId 的 anchor 在首次 plan 的原始时序位置变成可见 `plan_snapshot`；前端不能先 remove 再 add，也不能对从未可见的 timeline item做普通 update。即使所有 step 较早显示 completed，也继续接受该 turn 后续的 full replacement。历史 Plan 的 disclosureKey 为 `item:<planItemId>:details`。Plan 是底部投影，但不是 segment 顺序的例外。
- 思考摘要仍属于各自 reasoning stage，不能塞进 Plan bar。

## 6. Follow-up composer

- task queued/starting/running/cancelling/recovering 时禁用发送并显示当前状态；Turn 状态用 turnId 取消，恢复中用 activeRecoveryId 取消，不能拿 last turnId 代替 recovery target。
- task 为 idle 且 session 可继续时启用；Enter 发送，Shift+Enter 换行。
- `interrupted` 时禁用普通直接发送：read 会自动尝试 load；write 显示“恢复会话（不重放）”“重试中断提示”与 follow-up 明确动作。每个动作调用 conditional `task.continue`；resume 成功只回到 idle，不创建空 Turn，send/retry 才在 load 成功后原子创建新 turn。永不复活旧 turn 或静默重发 write prompt。
- 发送后立即插入用户消息并清空输入；后台失败则在该消息旁显示重试错误。
- follow-up 沿用 session 的 read/write 权限；Header 始终可见该模式。
- app 重启后若需 load，composer 显示“正在恢复 Grok 会话”，恢复完成前不可发送。

## 7. Bottom-follow 滚动状态机

常量建议：bottom threshold 48px，用户意图窗口 250ms。

状态：

```text
following-tail <-> detached-by-user
```

规则：

1. 首次打开某任务或用户明确切换到一个从未查看的任务时默认 following-tail，并在首次布局完成后校正到底部；关闭后重新打开同一 popover/task 必须恢复之前的 following/detached anchor 与未读数。
2. 新 item、文本增长、Markdown 重排、Plan/composer 高度变化时，只有 following-tail 才更新 scrollTop。
3. wheel、touchmove 或直接拖动滚动条产生用户意图；只有主 timeline 的 scrollTop 确实随该操作变化且离开底部阈值时才转 detached。thought/terminal 等内层 scroller 使用 `overscroll-behavior: contain`，其内部滚动不能解除主 bottom-lock。
4. 程序化布局变化不能触发 detached，也不能让 detached 自动重锁。
5. 用户带意图回到底部阈值内，或点击“回到最新”，转回 following-tail。
6. detached 时显示浮动“回到最新”按钮；有新 item 时显示未读计数。按钮不能遮挡 composer。
7. 展开/收起旧卡片视为用户布局操作，尽量用 scroll anchoring 保持触发项位置，不强制跳到底部。
8. detached 状态不能只保存裸 `scrollTop`；GUI host 保存 `{anchorItemId, intraItemOffsetPx, lastSeenSequence}`。渲染投影维护 `underlying itemId -> top-level virtual row key / member offset`：聚合形成、prepend、merge 或 split 前捕获 anchor 的屏幕 Y，重算后若 item 被收进折叠组就映射到 aggregate header，若组已展开则映射到平坦 member row，再补偿原 Y。Markdown reflow 或 `plan_finalize` 旧位置插入也走同一机制；item 真被删除时才退化到最近 sequence 邻居，只有无任何邻居才退到底部。

使用 `ResizeObserver` 观察 scroll container 与内容高度。首发必须使用窗口化/虚拟化列表，只重新测量被更新 item，并保持稳定 anchor；10,000 个 timeline item 下不能把所有重型详情挂载到 DOM。

## 8. Markdown 与内容安全

- 禁止执行 raw HTML、script、iframe、object、事件属性和 `javascript:` URL。
- 外部链接使用系统浏览器打开，显示真实域名提示；不在 WebView 内导航离开应用。
- 默认不自动加载远程图片；本地图片只允许经过 Tauri asset protocol 的受控路径。
- code block 提供复制按钮；terminal ANSI 转义必须使用安全 parser，不插入未经净化的 HTML。
- 超长内容使用显式“已截断/查看诊断”提示，不静默丢失。

## 9. 历史与 UI 状态

- 历史列表显示 title、cwd basename、mode、actual model、status、时间、duration。
- 支持按 title/cwd/result 文本搜索和按状态/mode 过滤。
- `ui_state` 按 `(taskId, disclosureKey)` 保存展开意图；清除任务时一起删除。
- popover 与完整窗口对同一 item 的手动展开意图实时同步。popover 对重型 diff/terminal 的 `user-expanded` 只展示该 surface 能承载的语义预览与“在完整窗口打开”，完整细节仍在主窗口；这不改变共享展开意图。
- scroll/follow/unread/draft 按 `(surfaceId, taskId)` 独立保存在 GUI host 内存中；detached scroll 使用稳定 anchor tuple，不是裸 scrollTop。这些不是全局展开状态，也不写入 SQLite。隐藏再打开同一 surface 恢复，应用完全退出后可以丢失。
- popover selection mode 为 `auto | user-pinned`。用户手动选 task 或将其视图滚成 detached 后转 user-pinned；只要该 task 仍存在，隐藏/新任务都不改选中项，新任务只增 badge/unread。auto 只在没有有效选择或 popover 隐藏后重新打开时选任务，排序为 `running|cancelling|recovering|interrupted` > `starting` > `queued` > `idle` > `failed|cancelled`，同级按 updatedAt 降序、taskId 升序；popover 可见期间即使 auto 也不被后台任务抢占。选中 task 被删除后回到 auto。每个 task 保留自己的 scroll/follow/unread。

## 10. 可访问性与国际化

- 首发提供简体中文和英文；所有运行状态与工具动词走 i18n key。
- 图标不能是状态的唯一表达；同时提供文字与 `aria-label`。
- 完整键盘导航：Tab 到卡片、Enter/Space 展开、Esc 关闭 popover、Cmd/Ctrl+K 聚焦历史搜索。
- 遵循系统 reduced-motion；shimmer 和平滑滚动在该模式下禁用。
- 浅色/深色跟随系统，也允许显式选择；错误、运行、成功颜色满足 WCAG AA 对比度。
- streaming token 不放入高频 `aria-live`；只公告阶段切换、工具完成、权限异常、错误、取消和最终回复完成。

## 11. 验收场景

必须使用合成 ACP fixture 和真实 Grok session 覆盖：

- Thought A → Tool 1 → Thought B → Assistant → Tool 2 → Assistant 的顺序完全一致。
- token-sized thought chunks 只形成一个阶段块，不形成数百事件行。
- tool update 只更新一张卡，update-before-create 也不重复。
- 用户不滚动时流式内容一直可见；上滚后视口稳定；回到底部恢复跟随。
- 用户手动展开 thought/tool 后，完成、刷新、切换窗口、历史回放都保持展开。
- 长用户消息、聚合行与历史 Plan 的用户展开/折叠在 snapshot 更新、成员增长和跨窗口切换后保持。
- 用户已展开的 read/search 在相邻新动作完成后不被聚合行替换。
- 聚合 prepend、auto→protected merge、两个 protected group 相邻时， disclosure state 与 detached anchor 均按规则保持。
- 10,000 个连续轻量动作聚合后，展开组仍由外层虚拟列表窗口化，单组不超过 100 项。
- Plan snapshot 更新不重复；`plan_finalize` 原子隐藏 bar 并在原位显示一个历史快照，detached anchor 不跳。
- Markdown 的中文、emoji、代码围栏、表格在 chunk 边界上仍正确。
- 正常 DOM 文本中不存在 `session/update`、`tool_call_update` 或完整 JSON object。
- 在 thought/terminal 内滚动不解除主 timeline bottom-lock。
- 10,000 个 item 加持续 streaming update 时，DOM 保持窗口化，detached anchor 不漂移。
- A→B→A 快速切换与两个 WebView 交错 snapshot 时，迟到 stream 不覆盖当前 selection epoch。
- 同 task 的 S1→S2 重订阅中，迟到 S1 end 不覆盖 S2，后台缓存也不回退 generation/sequence。
- B snapshot 缓慢/失败时 A 仍持续显示 live；B 成功才原子切换，A→B→C 不误清理 A。
- 100+ step active Plan 在 popover/完整窗口内可滚动且窗口化，不挤掉 timeline/composer。
