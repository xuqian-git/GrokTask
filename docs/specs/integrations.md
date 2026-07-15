# GrokTask Agent 集成、托盘设置与发布规格

状态：已确认的实现规格。

## 1. 集成目标

首发只管理两类用户级 MCP 集成：

- Codex
- Claude Code

每个目标只有两种 mode：

```text
none | mcp
```

GrokTask 不安装 Codex plugin、skill、hook 或 Claude command。MCP tool description 是调用方发现能力的权威入口。

## 2. Server identity

- MCP server key：`groktask`。
- command：当前正在运行的 `GrokTask` 可执行文件的绝对、规范化路径。
- args：`["mcp"]`。
- 不能写裸 `GrokTask` 依赖 PATH，因为桌面 Agent 的环境经常不继承交互 shell。

## 3. Codex 配置

目标文件：`~/.codex/config.toml`。

目标条目：

```toml
[mcp_servers.groktask]
command = "/absolute/path/GrokTask"
args = ["mcp"]
startup_timeout_sec = 30
tool_timeout_sec = 86400
```

只编辑 `[mcp_servers.groktask]`。使用 `toml_edit` 保留用户其它 section、注释、键序与格式。卸载后如果父表为空，可以删除空表；不得触碰其它 MCP server。

## 4. Claude Code 配置

目标文件：`~/.claude.json`，top-level `mcpServers.groktask`。

目标条目：

```json
{
  "command": "/absolute/path/GrokTask",
  "args": ["mcp"],
  "timeout": 86400000
}
```

`timeout` 单位为毫秒，使用 24 小时覆盖 Claude MCP 长任务默认超时。采用 JSON CST 最小编辑，保留文件其它字段与尽可能多的原格式；解析失败时中止并显示错误，绝不把大文件重新序列化覆盖。

## 5. 状态判定

每个 Agent 显示：

- `not_installed`：没有 groktask entry。
- `installed`：command、args 和 timeout 与当前模板一致。
- `outdated`：entry 存在，但 binary path、args 或 timeout 不一致。
- `invalid_config`：文件存在但无法安全解析，或目标父节点类型错误。
- `unavailable`：Agent 配置目录无法访问。

`outdated` 可以一键 Update；Install 与 Update 使用同一幂等 upsert。重复执行不得产生 diff。

## 6. CLI 与设置 UI

CLI：

```text
GrokTask agents status
GrokTask agents status codex
GrokTask agents mode codex mcp
GrokTask agents mode codex none
GrokTask agents mode claude mcp
GrokTask agents mode claude none
GrokTask agents workflow status [codex|claude]
GrokTask agents workflow enable codex|claude
GrokTask agents workflow disable codex|claude
```

`GrokTask setup` 打开单实例 Settings 窗口的 Integrations 页，不在命令行中偷偷修改配置。

### 6.1 协作指令（全局用户级）

工具开关的「协作指令」写入 **全局用户指令文件**（不是项目级）：

| Agent | 指令文件 |
| --- | --- |
| Codex | `~/.codex/AGENTS.md`（不写 `AGENTS.override.md`） |
| Claude Code | `~/.claude/CLAUDE.md` |

- 状态 / 启用 / 禁用不依赖项目 workspace 或 `--cwd`（`--cwd` 可保留兼容，但不参与目标解析）。
- 只编辑 GrokTask managed block；保留 AskHuman 与其它用户内容；标记畸形时拒绝写入。
- 需要时创建父目录（`.codex/`、`.claude/`）。
- MCP 配置路径不变：`~/.codex/config.toml`、`~/.claude.json`。

设置页每个 Agent card 显示：

- MCP 检测状态与安装/更新/移除；
- 协作指令状态与启用/禁用；
- 配置文件路径与全局指令文件路径（标注「指令文件（全局）」）；
- 将写入的当前 binary path；
- 修改后需要重启或重新载入 MCP / 新开会话的提示。

所有写操作在 UI 中显示预期影响；成功后重新读取文件验证，不只相信 write 返回值。

## 7. 安全编辑要求

- 先完整读取并解析，再生成最小变更。
- 解析失败、parent 不是 object/table、权限不足时不写任何字节。
- 在同目录写临时文件并 atomic replace；尽可能保留原文件权限。
- 写前后都只允许自有 `groktask` entry 发生语义变化，测试需比较其它节点。
- command path 作为数据写入，不经 shell escaping。
- Remove 本就不存在时成功 no-op。

## 8. 托盘设置与登录项

Settings > General 提供：

```text
托盘图标：关闭 / 任务活动时 / 始终显示
```

行为：

- `off`：移除 GrokTask 登录项；任务不会自动出现托盘图标。
- `active`：移除登录项；daemon 有任务时按需启动 GUI host，空闲退出。
- `always`：安装指向当前绝对 binary path 与 `--gui-host` 的用户登录项；GUI host 登录后常驻。

平台实现：

- macOS：LaunchAgent，使用稳定 label，更新 binary 后幂等更新 plist。
- Windows：当前用户 Startup/注册表登录项，不请求管理员权限。
- Linux：XDG autostart desktop entry；无图形会话时只保留配置，不把失败视为 MCP 不可用。

切换 mode 后立即协调当前 GUI host 的生命周期。登录项只由 `always` 创建，`active` 绝不自启动登录项。

## 9. 右键托盘菜单

菜单项目按平台原生渲染并随状态更新：

```text
GrokTask · 2 个任务运行中
当前：正在运行测试
-------------------------
打开当前任务
打开 GrokTask
历史
设置
-------------------------
Daemon：运行中
重启 Daemon
-------------------------
退出 GrokTask
```

- 顶部状态项不可点击或只用于打开当前任务。
- “退出 GrokTask”退出 GUI host；若有活动任务，任务继续在 daemon 中运行，并在确认文案中说明。
- “停止 daemon”不放在一级菜单，避免误杀任务；在设置/诊断中提供，有活动任务时需确认取消。

## 10. 依赖检测与登录

Grok CLI 状态：

- 未找到：显示官方安装指引，不自动安装。
- 找到但未登录：显示 `grok login` 命令与“在终端打开”操作，不自动发起交互登录。
- 版本不兼容：显示当前版本和最低测试版本；允许用户继续 doctor，但阻止任务并给出升级指引。
- 可用：显示 path、version、默认模型与最近检查时间。

GrokTask 不读取或存储 xAI token。ACP 子进程继承用户已有 Grok 本地认证环境。

## 11. 发布产物

CI 矩阵至少构建：

- macOS arm64 与 x86_64（可另提供 universal bundle）；
- Windows x86_64；
- Linux x86_64。

产物：

- macOS `.dmg` 或签名 `.app` 压缩包；
- Windows `.msi`/NSIS installer；
- Linux `.AppImage`，可另提供 `.deb`；
- 每个平台的裸 CLI-compatible `GrokTask` binary 用于自动化测试和高级安装。

每个 release 附 SHA-256 checksums。代码签名/notarization 在 secret 可用时启用；没有签名 secret 的 PR 构建仍应产出测试 artifact。

首发不要求自更新器。设置页可以显示当前版本和 release 链接，但不能静默下载执行。

## 12. Linux 桌面降级

“完整支持”包含 daemon、MCP、历史、完整窗口和 tray-capable 桌面；但 Linux 托盘由桌面环境决定：

- 支持 StatusNotifier/AppIndicator 时展示托盘与锚定 popover。
- tray host 没有可靠左键 activate event 时，菜单中的“打开当前任务/打开浮层”提供同等入口，doctor 显示 `tray_click: unavailable`，不把它误报为完整左键能力。
- 只有传统 tray 时使用 Tauri 可用实现。
- 完全没有 tray host 时，`GrokTask app` 仍可打开完整窗口，`doctor` 明确报告 tray unavailable；任务功能不失败。

不能因 tray unavailable 启动 localhost 作为替代。

## 13. 从旧 plugin 迁移

- 仓库删除 `.codex-plugin`/plugin skills/hooks/Node companion/localhost dashboard 与其测试。
- README 改为 GrokTask 安装、MCP 配置、运行与开发文档。
- 保留 `docs/research/acp-conversation-flow.md` 作为设计来源。
- 不自动删除用户 `~/.codex/plugins/cache` 中旧版本，避免修改 Codex 管理目录；README 给出通过 Codex 正常卸载旧 plugin 的说明。
- 如果检测到旧 grok-codex MCP/plugin entry，只提示可能重复调用，不擅自删除。
