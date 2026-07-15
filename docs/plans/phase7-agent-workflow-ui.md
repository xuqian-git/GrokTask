# Phase 7 Agent workflow injection and Chinese desktop UX brief

Status: ready to delegate to Grok. Codex owns this brief and review; Grok owns all product/test code.

## Why this phase exists

The current GrokTask implementation is a standalone MCP task runner. It can install a `groktask` MCP server for Codex and Claude Code, run Grok tasks, and show per-task ACP timelines. That is useful, but it is not the product target.

The product target is closer to AskHuman:

- the desktop app lets the user enable/disable GrokTask for Codex and Claude Code;
- enabling does more than install MCP: it injects clear managed instructions so the host agent proactively calls GrokTask during coding;
- Codex/Claude own requirement understanding, planning, architecture, review, bug diagnosis, and performance analysis; after they produce/update a plan/spec/checklist or diagnosis, they use GrokTask for coding implementation, file changes, test additions, and fix implementation until the requirement is done;
- the app UI is Chinese by default and feels like a native local tool;
- the menu-bar popover shows live ACP activity;
- the full window can browse all ACP/task records, not only a single task detail.

This phase corrects the product direction without reviving the old Codex plugin architecture. GrokTask remains a standalone binary and MCP server.

## References

- `/Users/qian/project/AskHuman` is the primary UX reference for:
  - agent enable/disable switches;
  - managed instruction blocks;
  - blocking-tool ergonomics;
  - local desktop menu-bar behavior.
- Current code:
  - `src/views/SettingsView.vue`
  - `src/views/HistoryView.vue`
  - `src/views/TaskView.vue`
  - `src/views/PopoverView.vue`
  - `src-tauri/src/integrations/`
  - `src-tauri/src/app/gui_host.rs`
  - `src-tauri/src/app/tray.rs`
  - `src-tauri/src/app/windows.rs`
  - `src-tauri/src/app/mod.rs`
  - `src-tauri/src/cli/mod.rs`
  - `src-tauri/src/mcp/`
  - `src-tauri/src/daemon/task_manager.rs`
- Existing specs to update or follow:
  - `docs/specs/product.md`
  - `docs/specs/integrations.md`
  - `docs/specs/cli-mcp.md`
  - `docs/specs/conversation-stream.md`
  - `docs/acceptance.md`

## Product model

GrokTask has two separate integration layers per host agent:

1. **MCP Server**
   - Installs/removes the `groktask` MCP server entry.
   - This only makes tools available.
   - It does not by itself change the agent's behavior.

2. **Workflow Instructions**
   - Adds/removes a managed instruction block for Codex or Claude Code.
   - This tells the agent when and how to call GrokTask proactively.
   - This is the missing layer in the current implementation.

Settings must show both layers independently, because advanced users may want MCP installed without workflow injection.

## Managed instruction targets

Implement safe managed-block injection. Do not silently rewrite unrelated user content.

Targets are **global user instruction files** (not project-level). Settings → 工具开关 enables a default workflow for Codex and Claude Code across projects.

### Codex

Primary target (Codex home; default `~/.codex`, or under the configured/test home root):

```text
<home>/.codex/AGENTS.md
```

Codex also supports `AGENTS.override.md`; this app only manages `AGENTS.md` and never writes `AGENTS.override.md`.

If the file does not exist, create it (including parent `.codex/` as needed).

If the file exists, preserve all user content and insert/update only the managed block:

```markdown
<!-- GrokTask:begin DO NOT EDIT (managed by GrokTask) -->
...
<!-- GrokTask:end -->
```

Do not remove or alter AskHuman managed blocks.

### Claude Code

Primary target (user-level, Anthropic docs):

```text
<home>/.claude/CLAUDE.md
```

Use the same managed block markers:

```markdown
<!-- GrokTask:begin DO NOT EDIT (managed by GrokTask) -->
...
<!-- GrokTask:end -->
```

MCP install config remains unchanged: Codex `~/.codex/config.toml`, Claude `~/.claude.json`.

### Scope

Workflow status/enable/disable are global-user scoped via `IntegrationRoots` (tests use temp homes). They must not require a project workspace/`--cwd` for target resolution. The UI labels the path as global/user-level and must not claim project-level injection for this feature.

## Default managed instruction content

The injected block should be concise, forceful, and operational. It must teach the host agent (Codex / Claude Code) to own planning and to use GrokTask as an execution collaborator—not a planning replacement.

Use Chinese user-facing language where appropriate, but the instruction content may be bilingual if that helps the agent.

Suggested content (must stay in sync with `DEFAULT_WORKFLOW_BODY` in `src-tauri/src/integrations/workflow.rs`):

```markdown
## GrokTask 协作协议

当用户要求写代码、改代码、补测试或按既定方案实现修复时，默认使用 GrokTask，把执行型编码工作委派给 Grok Build。你必须优先考虑调用 `groktask` MCP；除非命中下方“明确豁免”，不要因为“自己也能写”就跳过 GrokTask。

### 角色分工

- **你（Codex / Claude Code）**：负责理解需求、规划、架构决策、review、bug 排查、性能分析，并在需要时生成/更新 plan、spec、诊断结论或 checklist。不要把规划、分析或最终判断交给 Grok。
- **Grok**：按你给出的 plan/文档/spec/诊断结论与验收标准执行编码、修改文件、补测试或实现修复；不替代你的规划、分析、review 与最终判断。

### 默认触发

以下情况默认必须使用 GrokTask：

- 需要写代码或修改代码，且不是 1–2 行的微小机械改动；
- 跨文件编码实现、重构执行、迁移执行、UI 实现、测试补齐或构建修复落地；
- 你已经完成需求理解、规划、bug 根因分析或性能分析，下一步是按结论改代码；
- 你已经写出 plan/spec/checklist/诊断结论，下一步进入编码执行。

以下情况不要委派给 GrokTask，由你自己完成：

- review、第二意见、方案判断、架构取舍；
- bug 排查、失败测试分析、根因定位；
- 性能分析、稳定性分析、安全风险分析；
- 需求澄清、规划、spec/checklist 编写。

### 明确豁免

只有以下情况可以不调用 GrokTask，并继续自己处理：

- 纯解释、问答、总结、翻译、文档阅读，不需要代码改动；
- 微小改动：例如改一个文案、修一个 typo、调整 1–2 行显然代码；
- 当前任务还处于 review、bug 排查、性能分析或规划阶段，尚未形成明确编码任务；
- 任务涉及 secrets、token、隐私数据，不能安全发送给 Grok；
- GrokTask 不可用、返回失败、连续两轮修复仍不收敛；
- 用户明确要求“不要用 Grok/GrokTask”。

### 默认循环

1. 先理解用户需求与当前仓库状态。review、bug 排查、性能分析由你负责；需要时先产出 plan/spec/checklist/诊断结论。
2. 当下一步是明确的编码执行时，带着计划/诊断结论、相关文件路径与具体验收标准调用 GrokTask：
   - 需要 Grok 修改代码、补测试或落地修复时使用 `run` 或 `start`，`mode` 设为 `write`；
   - 只允许在需要 Grok 读取代码并准备编码上下文时使用 `mode` 设为 `read`；不要把 review、bug 排查或性能分析委派给 Grok。
3. Grok 返回代码改动/结果后，你必须 review 并验证，不要盲目信任。
4. 若 review 发现问题，再调用 GrokTask 让 Grok 按审查结论修复；然后继续 review。
5. 重复「Grok 执行 → 你 review/验证 → Grok 修复」，直到没有阻塞问题，或必须让用户做产品/权限决策。
6. 你负责最终验证、总结和交付。

### 约束

- 不要为了微小、显然可直接完成的改动调用 GrokTask；但不要把“我能自己做”当成跳过理由。
- 不要让 Grok 自动提交、推送、创建 PR 或扩大工作目录，除非用户明确要求。
- 不要把 secrets、token、个人敏感信息发送给 GrokTask。
- 如果 GrokTask 返回失败、取消或部分结果，如实报告并决定下一步。
- 如果连续两轮修复仍无法收敛，停止循环并向用户说明阻塞点。
```

The exact text can be refined, but it must preserve this host-plans / Grok-executes behavior and the safety constraints.

## UI requirements

The UI must default to Simplified Chinese. English-only labels like "Settings", "Integrations", "History", "Phase 5", "Install", "Remove" should be replaced.

### Main surfaces

Use three user-facing concepts:

1. **工具开关**
   - Shows Codex and Claude Code cards.
   - Each card has:
     - MCP server status and install/remove/update button;
     - workflow instruction status and enable/disable/update button;
     - target config/instruction file path;
     - current binary path;
     - clear post-change guidance.
   - This replaces the current English-only Integrations page as the primary Settings page.

2. **ACP 记录**
   - Shows all tasks/turns in a readable global activity/history view.
   - The user should be able to see recent ACP/Grok records across all tasks, then open a task detail.
   - Do not show raw ACP JSON in normal view.
   - Show semantic items: user prompt, thought/reasoning summary, tool action, file/action text, final answer, error/cancel status.

3. **菜单栏实时面板**
   - macOS menu-bar/status item should be visible when tray mode is `active` or `always`.
   - Left-click opens an anchored popover with the current/latest live ACP record stream.
   - Right-click opens a native menu.
   - The popover must not look like a detached random full window. It should be compact, Chinese, and focused on live activity.

### Settings tab bug

Currently clicking Settings tabs changes visible content but the URL/focus can remain on `section=integrations`, creating inconsistent “click twice” behavior. Fix this:

- clicking a Settings tab updates the internal state, visible content, and URL/query consistently;
- opening `GrokTask setup` with `section=integrations` must select the integrations/tools section exactly once;
- repeated clicks must not be required.

### History page

The current history page is too placeholder-like. Improve it:

- Chinese labels;
- clear grouping by time/status;
- useful empty/error/loading states;
- clicking a record opens the full task detail reliably;
- search/filter controls should not dominate the page when there are only a few tasks;
- display all available tasks from daemon, not fixture/demo records.

### Navigation

The current app can end up showing popover content when the user expects the full window. Fix/clarify:

- full app window should have the full navigation shell;
- popover should have compact popover layout;
- "完整窗口" from popover must open/focus the full window;
- CLI `GrokTask app` should open the full window, not leave the user stuck in popover.

## Backend requirements

### Integration status DTOs

Extend status reporting so each agent has two statuses:

```text
mcp: not_installed | installed | outdated | invalid_config | unavailable
workflow: not_enabled | enabled | outdated | invalid_file | unavailable
```

Keep old fields if needed for compatibility, but frontend should display both layers.

### New commands / CLI

Add backend support for workflow instruction management:

```text
GrokTask agents workflow status [codex|claude] [--cwd PATH]
GrokTask agents workflow enable codex|claude [--cwd PATH]
GrokTask agents workflow disable codex|claude [--cwd PATH]
```

The UI can call equivalent Tauri commands.

`--cwd` may remain accepted for backward compatibility but is **not** used to resolve global instruction targets. Workflow status and enable/disable work without a workspace. Paths shown in CLI/UI are always the global files under the integration home (`~/.codex/AGENTS.md`, `~/.claude/CLAUDE.md`).

### Safety

- Managed block operations must be idempotent.
- Enabling twice must produce no diff.
- Disabling removes only the GrokTask block.
- If a file contains a malformed begin/end marker pair, do not write; show a clear error.
- Preserve user content, line endings as reasonably as possible, and final newline.
- Never edit AskHuman managed blocks.

## Tests required

Add deterministic tests for:

1. Managed block injection
   - create missing global `~/.codex/AGENTS.md` / `~/.claude/CLAUDE.md` under temp home;
   - append to existing file;
   - update old GrokTask block;
   - disable removes only GrokTask block;
   - malformed marker refuses to write;
   - AskHuman block is preserved;
   - never writes `AGENTS.override.md`.

2. Integration status
   - MCP installed but workflow disabled;
   - MCP installed + workflow enabled;
   - workflow outdated;
   - invalid instruction file;
   - status reports global paths without workspace.

3. CLI
   - workflow status/enable/disable routes work with temp IntegrationRoots home;
   - no real user config / real home instruction files are touched during tests.

4. Frontend
   - Chinese labels render by default;
   - tools page shows MCP and workflow switches separately;
   - Settings tab click changes selected section without requiring a second click;
   - History/ACP records page renders task list and opens details;
   - Popover “完整窗口” opens/focuses full layout path.

5. Existing regression gates
   - `pnpm format:check`
   - `pnpm test`
   - `pnpm build`
   - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
   - `cargo test --manifest-path src-tauri/Cargo.toml --all-features`
   - `pnpm tauri build --bundles app`

## Implementation notes

- Code changes are delegated to Grok.
- Do not remove the existing MCP install/remove functionality.
- Do not change GrokTask into a Codex plugin.
- Do not add localhost dashboard back.
- Do not auto-enable workflow instructions when the user only installs MCP.
- Write only the GrokTask managed block in the documented global instruction files; never rewrite unrelated user content or `AGENTS.override.md`.
- Prefer simple, reliable UI over decorative complexity.

## Acceptance criteria

This phase is acceptable when:

- a user can open GrokTask, see Chinese UI, and clearly enable/disable Codex/Claude MCP plus global workflow instructions;
- the global target `~/.codex/AGENTS.md` / `~/.claude/CLAUDE.md` receives a safe GrokTask managed block;
- a host agent reading that block would know it must plan first for non-trivial work, pass plan/spec and acceptance criteria to Grok, then own review/verify/fix loops;
- the app has an ACP records/history view that is useful without raw JSON noise;
- macOS menu-bar popover opens and shows recent/live activity;
- Settings tab navigation is single-click reliable;
- tests and packaged app build pass;
- Codex review finds no blocking issues.
