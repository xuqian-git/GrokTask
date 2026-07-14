# GrokTask 产品规格

状态：已确认的实现规格。

## 1. 产品定义

GrokTask 是一个独立、跨平台、单二进制的本地工具。Codex 或 Claude Code 通过 MCP 直接调用它，把编码、诊断、审查或重构任务交给 Grok Build；用户通过原生桌面窗口和系统托盘查看真实的执行过程、继续对话或取消任务。

GrokTask 不是 Codex 插件，也不依赖浏览器、localhost 页面或 Node.js 插件运行时。

## 2. 首发平台

- macOS：菜单栏图标、锚定式对话浮层、完整窗口、登录项。
- Windows：系统托盘图标、锚定式对话浮层、完整窗口、登录项。
- Linux：系统托盘图标、锚定式对话浮层、完整窗口；在桌面环境无法提供托盘定位信息时，浮层定位到当前显示器右上方并保持同等功能。

三端共享同一套 Rust 核心、ACP reducer、持久化格式和 Vue 界面。

## 3. 主要用户旅程

### 3.1 Agent 阻塞调用

1. Codex 或 Claude Code 调用 MCP `run`，显式传入 `mode: read | write`、任务描述和工作目录。
2. GrokTask daemon 创建任务并启动 Grok ACP 会话。
3. MCP 调用持续等待；过程实时写入本地时间线。托盘为 `active/always` 或用户已经打开应用时，浮层和完整窗口同步实时显示；默认 `off` 时任务不会为了展示而主动启动 GUI。
4. Grok 完成后，MCP 返回最终或明确标注的部分 Markdown 回复和结构化任务元数据。
5. 调用方继续审查工作区或向用户报告。

### 3.2 Agent 异步调用

1. 调用方为一次逻辑提交生成并在 retry 时复用 `submissionId`，使用 `start` 获取 `taskId + turnId`；response 丢失重试不会创建第二个任务。
2. GrokTask 在 daemon 中继续运行，不依赖发起调用的 stdio 连接存活。
3. 调用方保存 `taskId + turnId`，用 `status` 或 `wait` 获取进度与该轮最终结果，需要时用 conditional `cancel` 取消该轮。

### 3.3 用户在浮层中跟进

1. 用户左键点击托盘图标，打开锚定浮层。
2. 浮层显示当前任务的严格时序对话流、当前计划步骤和实际模型。
3. task 为 idle 时，用户可在输入框发送 follow-up；若为 interrupted，“恢复会话”只 load 到 idle，不创建空 Turn，“发送/重试”才在 load 成功后创建新 turn。GrokTask 在同一 ACP session 上再次调用 `session/prompt`。
4. 任务运行时输入框禁用，避免同一 session 并发 prompt；用户仍可取消。

### 3.4 查看历史

1. 用户从右键菜单或浮层打开完整窗口。
2. 左侧查看最近任务，右侧查看完整语义时间线。
3. 默认保留最近 200 个任务；设置中可修改数量或清空历史。

## 4. 已确认的产品决策

| 项目 | 决策 |
| --- | --- |
| 名称与可执行文件 | `GrokTask` |
| 交付形态 | Tauri 2 单二进制；CLI、MCP、daemon、GUI host 为同一程序的不同角色 |
| Agent 集成 | Codex 与 Claude Code 的用户级 MCP 配置 |
| MCP 主入口 | 阻塞 `run`；另有 `start`、`status`、`wait`、`cancel` |
| 权限模式 | 每次 `run` / `start` 必须显式传 `read` 或 `write`，不存在隐式默认值 |
| UI | 只使用原生 Tauri WebView；不提供 localhost 镜像 |
| 托盘交互 | 左键打开锚定实时对话浮层；右键打开原生菜单 |
| 托盘可见性 | `off | active | always`，默认 `off`；行为参考 AskHuman |
| 对话模型 | 严格时序语义流；Thought、Tool、Thought、Reply 保持真实顺序 |
| 思考展示 | 每个连续阶段独立；流式三行预览，完成后一句摘要；手动展开状态不被覆盖 |
| Plan | 时间线与输入框之间始终展开完整步骤；完成后在首次出现位置保留一次快照 |
| Markdown | assistant 与 reasoning 均支持安全 Markdown；原始 HTML 不执行 |
| 滚动 | 用户未主动离开底部时自动跟随；手动上滚后暂停，回到底部后恢复 |
| 历史 | 完整本地语义流默认保留 200 个任务，可配置、可清空 |
| 模型 | 默认继承 Grok CLI 当前默认；支持每次调用覆盖；UI 显示实际模型 |
| 本机实测默认模型 | Grok 0.2.101 当前为 `grok-4.5`；不在代码中硬编码该值 |

## 5. 模式语义

### 5.1 `read`

- 只允许读取、搜索和 Grok 内建白名单中的只读 Git 命令。
- 使用 Grok `read-only` sandbox、`dontAsk` permission mode、禁用 WebFetch、编辑和 subagent；不添加可匹配 `git push/reset/clean` 的宽泛 Bash allow rule。
- 适合代码审查、诊断、方案分析和第二意见。
- 如果 Grok 尝试写入，任务应失败并在时间线显示清晰错误，不能静默切换为 write。

### 5.2 `write`

- 明确允许 Grok 修改传入工作目录内的文件。
- 使用 Grok `workspace` sandbox 与非交互批准模式，避免 ACP 任务停在 TUI 权限提示。sandbox 另外只允许 Grok 自身使用 `/tmp` 与 `~/.grok`，不得写用户其它目录。
- UI 和 MCP 结果都必须显示这是 write 任务。
- GrokTask 不替调用方自动提交、推送、创建 PR 或扩大工作目录。

## 6. 正常视图与诊断视图

正常视图只回答用户关心的问题：Grok 正在思考什么、正在做什么、结果是什么。

以下内容不得进入正常对话流：

- `session/update` 等 ACP 方法名；
- 完整通知对象；
- `_x.ai/*` 原始方法名和 payload（Rust normalizer 可以提取其中已识别的阶段标题、模型或工具文本，再作为标准语义 item 展示）；
- 重复的 available command 列表；
- 原始 tool input/output JSON；
- 仅用于运行时的心跳、hook 和队列事件。

独立诊断页可以显示经过截断与凭证脱敏的原始 ACP 事件、进程 stderr 和生命周期日志，用于排障。

## 7. 非目标

- 不实现 Grok 本身的登录、付费、模型服务或账号管理；只检测并提示使用官方 `grok login`。
- 不实现通用 ACP 多 Agent 客户端；首发只运行 Grok CLI，但内部边界保持 ACP 标准化。
- 不取代 Git 客户端，不自动提交或推送。
- 不提供远程 Web 控制台、网络服务器或云同步。
- 不把旧 Codex plugin 作为兼容入口继续发布。
- 不在首发版本让 Codex/Claude 或桌面用户手动点击 ACP 权限按钮；异常 permission request 必须由 daemon 按 read/write 策略立即回应，绝不能悬挂。

## 8. 成功标准

- Codex 和 Claude Code 都能通过安装后的 MCP server 完成 read/write 任务。
- 用户在终端直接运行 Grok 能看到的主要 Thought、Tool 和 Reply 顺序，在 GrokTask 中不丢失、不重复、不被 JSON 噪声淹没。
- MCP 任务与 UI 同源：最终回复、状态、模型、取消结果保持一致。
- macOS、Windows、Linux 的 release 构建和核心 E2E 均通过。
- 旧 plugin 与 localhost dashboard 被完全移除，仓库交付的是独立 GrokTask 应用。
