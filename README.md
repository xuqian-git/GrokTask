<div align="center">

<img src="src-tauri/icons/128x128@2x.png" width="128" height="128" alt="GrokTask logo" />

# GrokTask

**Delegate planned coding work to Grok Build — watch it live, locally.**

Codex or Claude Code plans; GrokTask runs Grok Build via MCP; you follow the real **Thought → Tool → Reply** stream in a native desktop UI.

[English](#english) · [简体中文](#简体中文)

<br />

[![CI](https://github.com/xuqian-git/GrokTask/actions/workflows/ci.yml/badge.svg)](https://github.com/xuqian-git/GrokTask/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![GitHub stars](https://img.shields.io/github/stars/xuqian-git/GrokTask?style=social)](https://github.com/xuqian-git/GrokTask/stargazers)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey)](https://github.com/xuqian-git/GrokTask)
[![Stack](https://img.shields.io/badge/stack-Tauri%202%20%7C%20Rust%20%7C%20Vue-informational)](https://github.com/xuqian-git/GrokTask)
[![Release workflow](https://img.shields.io/badge/release-GitHub%20Actions-purple)](https://github.com/xuqian-git/GrokTask/actions/workflows/release.yml)

</div>

---

## English

### What it is

GrokTask is a **standalone, cross-platform local task runner**. The same `GrokTask` binary exposes CLI, MCP, a local daemon, and a native Tauri desktop UI (menu bar / tray popover and full window).

It is **not** a Codex plugin. It does not depend on a browser, a localhost dashboard, or a Node.js plugin runtime.

| You keep… | GrokTask handles… |
| --- | --- |
| Planning, review, diagnosis (Codex / Claude Code) | Implementation runs against Grok Build via MCP |
| Explicit `read` / `write` intent on every task | Blocking `run` or async `start` / `status` / `wait` / `cancel` |
| Final judgment on the workspace | Live Thought → Tool → Reply timeline in a native UI |

### Why it helps

- **Blocking agent calls** — MCP/CLI `run` waits until the turn completes (AskHuman-style).
- **Async control** — `start` / `status` / `wait` / `cancel` survive process restarts via a local daemon.
- **Explicit safety** — every task requires `mode: read` or `mode: write` (no defaults, no text inference).
- **Native visibility** — timeline, plan bar, history, and settings without Chrome or an in-app browser page.

### Prerequisites

**Runtime (end users)**

- **Grok CLI / Grok Build** installed and authenticated (`grok login` or the official auth flow; see [xAI docs](https://docs.x.ai)).
- A desktop session for tray / menu-bar UI (MCP and CLI work without a GUI).

**Development**

| Tool | Notes |
| --- | --- |
| Node.js | ≥ 20 |
| pnpm | 9.x (see `packageManager` in `package.json`) |
| Rust | stable, matching `rust-version` in `src-tauri/Cargo.toml` |
| Platform deps | Linux needs WebKitGTK and related packages for Tauri (see [CI workflow](.github/workflows/ci.yml)) |

### Quick start (build from source)

There is no published package-manager install yet. Build from this repository:

```bash
pnpm install --frozen-lockfile
pnpm build
cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol
# or full Tauri compile without OS installers:
pnpm tauri build --no-bundle
```

Binary after a cargo release build:

```text
src-tauri/target/release/GrokTask
# or, for a specific target:
src-tauri/target/<triple>/release/GrokTask
```

Production builds **must** use `--features custom-protocol` so frontend assets are embedded. Prefer `pnpm tauri build --no-bundle` for a full Tauri release compile without packaging. The form `pnpm tauri build -- --no-bundle` is incorrect for this CLI.

### Usage

#### Roles (same binary)

```bash
./src-tauri/target/release/GrokTask --help
./src-tauri/target/release/GrokTask --version
./src-tauri/target/release/GrokTask mcp            # stdio MCP; does not start Tauri
./src-tauri/target/release/GrokTask daemon run     # no WebView
./src-tauri/target/release/GrokTask daemon status
./src-tauri/target/release/GrokTask doctor
./src-tauri/target/release/GrokTask app            # open desktop UI / ensure GUI host
```

Hidden internal roles: `--gui-host`, `--task-supervisor` (not for everyday use).

#### CLI tasks

Mode is always explicit (`read` or `write`). There is no default.

```bash
# Blocking: waits until Grok returns a final / partial / cancelled / failed result
GrokTask run --mode read --cwd /absolute/path "Summarize this repo"

# Async start then poll / wait
GrokTask start --mode write --cwd /absolute/path --submission-id <uuid> "Apply the fix"
GrokTask status <taskId>
GrokTask wait <taskId> <turnId> [--timeout SECONDS]
GrokTask cancel <taskId> --turn <turnId>
```

| Mode / command | Behavior |
| --- | --- |
| `run` | Blocks until the turn finishes. Treat as a long-running tool call. |
| `read` | Read-only sandbox expectations; workspace should not be modified. |
| `write` | Grok may edit files under `cwd`. GrokTask does not auto-commit, push, or open PRs. Sandbox limits are not an unbreakable security boundary — treat write mode as trusted local automation. |

#### MCP (Codex / Claude)

```bash
GrokTask mcp
```

Server name: `groktask`. Tools only: `run`, `start`, `status`, `wait`, `cancel`. No UI resource, no localhost URL, no MCP Apps template.

Install agent config from **Settings → Integrations** (install / remove for Codex or Claude), or CLI:

```bash
GrokTask agents status
GrokTask agents mode codex mcp     # install / update GrokTask MCP entry
GrokTask agents mode claude mcp
GrokTask agents mode codex none    # remove GrokTask entry only
GrokTask agents mode claude none
```

Config editors only touch the GrokTask MCP server block; other servers and comments are preserved when the file is valid.

#### Desktop UI

| Platform | Behavior |
| --- | --- |
| macOS | Menu bar icon; left-click opens an anchored popover; right-click opens a native menu. |
| Windows / Linux | System tray with the same pattern (Linux may fall back when tray hosts are limited; see `doctor`). |

Popover and full window share the same task timeline and expansion state. Tray visibility: `off` \| `active` \| `always` (login item only for `always`). Default is `off` so background MCP work does not force a GUI up.

#### Doctor

```bash
GrokTask doctor
```

Reports binary paths, Grok availability, daemon/GUI host state, tray capability, and agent integration status. Useful when MCP works but the tray is missing on Linux.

### Data paths and safety

Default home (override with `GROKTASK_HOME` for isolation/tests):

```text
~/.groktask/
  config.json
  history.sqlite3
  daemon.lock / daemon.json / daemon.sock
  gui-host.lock / gui-host.sock
  daemon.log
  gui.log
```

Do not point automated tests at a real user `~/.groktask` without an explicit temp home.

### Development and verification

Frontend:

```bash
pnpm install --frozen-lockfile
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm build
```

Rust:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features
cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol
```

Tauri (no installer packages):

```bash
pnpm tauri build --no-bundle
```

Frontend gates only: `pnpm ci:frontend`.

Full product acceptance criteria (including manual Grok smoke tests) live in [`docs/acceptance.md`](docs/acceptance.md). Real Grok E2E is **manual / opt-in** and is not required for ordinary CI.

| Document | Path |
| --- | --- |
| Specs | [`docs/specs/`](docs/specs/) |
| Standalone refactor plan | [`docs/plans/standalone-refactor.md`](docs/plans/standalone-refactor.md) |
| Activity app migration note | [`docs/grok-activity-app.md`](docs/grok-activity-app.md) |

### Release status

CI quality gates run on push/PR (frontend + multi-target Rust). A separate **release** workflow (`workflow_dispatch` and version tags) builds **unsigned** binaries for:

| Target |
| --- |
| `aarch64-apple-darwin` |
| `x86_64-apple-darwin` |
| `x86_64-pc-windows-msvc` |
| `x86_64-unknown-linux-gnu` |

Each artifact is named with **target and version**, and is uploaded with a **SHA-256** checksum. Artifacts are **not notarized** and are **not** wrapped as platform installers yet — no secrets, code signing, or store publishing are wired in this repository.

There may be **no published GitHub Releases** yet; use Actions artifacts or build from source. See [`.github/workflows/release.yml`](.github/workflows/release.yml).

### Migration from the old Codex plugin

If you previously installed the experimental **Codex plugin** from this repository (skills, hooks, Node companion, localhost activity page):

1. **Uninstall the old plugin** through Codex’s normal plugin uninstall / disable flow.
2. **Remove any leftover local plugin checkout** if you still have an old `plugins/grok-codex` copy (this repo no longer ships that tree after Phase 6).
3. **Install GrokTask MCP** via Settings → Integrations or `GrokTask agents mode codex mcp` / `claude mcp`.
4. Prefer a **single** Grok integration: if an old plugin MCP entry remains, doctor/status may warn about duplicates; GrokTask will not delete other tools’ config without an explicit remove.

The old **localhost dashboard** is replaced by the native popover and full window. See [`docs/grok-activity-app.md`](docs/grok-activity-app.md).

### License

- **MIT** — see [`LICENSE`](LICENSE).
- Design inspiration from open-source ACP clients is conceptual only; this project does not copy GPL code.
- No third-party source was copied into this tree in a way that currently requires additional copyright notices beyond the MIT license and normal dependency licenses from crates/npm packages.

---

## 简体中文

### 它是什么

GrokTask 是一个**独立、跨平台的本地任务运行器**。同一个 `GrokTask` 二进制提供 CLI、MCP、本地 daemon，以及原生 Tauri 桌面界面（菜单栏 / 托盘浮层与完整窗口）。

它**不是** Codex 插件，也不依赖浏览器、localhost 看板或 Node.js 插件运行时。

| 你继续负责… | GrokTask 负责… |
| --- | --- |
| 规划、审查、诊断（Codex / Claude Code） | 通过 MCP 把实现交给 Grok Build |
| 每次任务显式声明 `read` / `write` | 阻塞式 `run`，或异步 `start` / `status` / `wait` / `cancel` |
| 对工作区结果做最终判断 | 在原生 UI 中展示真实的 Thought → Tool → Reply 时间线 |

### 解决什么问题

- **阻塞式 Agent 调用** — MCP/CLI 的 `run` 会等到本轮结束（类似 AskHuman）。
- **异步控制** — `start` / `status` / `wait` / `cancel` 通过本地 daemon 跨进程重启仍可用。
- **显式安全模式** — 每次任务必须传 `mode: read` 或 `mode: write`（无默认值、不从文案推断）。
- **原生可见性** — 时间线、计划条、历史与设置，无需 Chrome 或应用内浏览器页。

### 前置条件

**运行时（终端用户）**

- 已安装并完成认证的 **Grok CLI / Grok Build**（`grok login` 或官方认证流程；见 [xAI 文档](https://docs.x.ai)）。
- 需要托盘 / 菜单栏 UI 时使用桌面会话（MCP 与 CLI 可不依赖 GUI）。

**开发环境**

| 工具 | 说明 |
| --- | --- |
| Node.js | ≥ 20 |
| pnpm | 9.x（见 `package.json` 中的 `packageManager`） |
| Rust | stable，与 `src-tauri/Cargo.toml` 中 `rust-version` 一致 |
| 平台依赖 | Linux 需 WebKitGTK 等 Tauri 相关依赖（见 [CI 工作流](.github/workflows/ci.yml)） |

### 快速开始（从源码构建）

目前尚无包管理器上的正式安装发布。请从本仓库构建：

```bash
pnpm install --frozen-lockfile
pnpm build
cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol
# 或完整 Tauri 编译且不打系统安装包：
pnpm tauri build --no-bundle
```

cargo release 构建后的二进制路径：

```text
src-tauri/target/release/GrokTask
# 指定 target 时：
src-tauri/target/<triple>/release/GrokTask
```

生产构建**必须**启用 `--features custom-protocol`，以便嵌入前端资源。完整 Tauri 编译且不打包安装器时，请使用 `pnpm tauri build --no-bundle`。`pnpm tauri build -- --no-bundle` 对本 CLI 不正确。

### 使用说明

#### 同一二进制的多种角色

```bash
./src-tauri/target/release/GrokTask --help
./src-tauri/target/release/GrokTask --version
./src-tauri/target/release/GrokTask mcp            # stdio MCP；不启动 Tauri
./src-tauri/target/release/GrokTask daemon run     # 无 WebView
./src-tauri/target/release/GrokTask daemon status
./src-tauri/target/release/GrokTask doctor
./src-tauri/target/release/GrokTask app            # 打开桌面 UI / 确保 GUI host
```

内部隐藏角色：`--gui-host`、`--task-supervisor`（日常无需使用）。

#### CLI 任务

模式始终显式（`read` 或 `write`），没有默认值。

```bash
# 阻塞：直到 Grok 返回 final / partial / cancelled / failed
GrokTask run --mode read --cwd /absolute/path "Summarize this repo"

# 异步启动后再查询 / 等待
GrokTask start --mode write --cwd /absolute/path --submission-id <uuid> "Apply the fix"
GrokTask status <taskId>
GrokTask wait <taskId> <turnId> [--timeout SECONDS]
GrokTask cancel <taskId> --turn <turnId>
```

| 模式 / 命令 | 行为 |
| --- | --- |
| `run` | 阻塞直到本轮结束，应按长耗时工具调用处理。 |
| `read` | 只读沙箱预期，不应修改工作区。 |
| `write` | Grok 可在给定 `cwd` 下改文件。GrokTask **不会**自动 commit、push 或开 PR。沙箱不是牢不可破的安全边界——请将 write 视为受信任的本机自动化。 |

#### MCP（Codex / Claude）

```bash
GrokTask mcp
```

服务名：`groktask`。仅工具：`run`、`start`、`status`、`wait`、`cancel`。无 UI 资源、无 localhost URL、无 MCP Apps 模板。

通过 **设置 → Integrations** 安装/移除 Codex 或 Claude 配置，或使用 CLI：

```bash
GrokTask agents status
GrokTask agents mode codex mcp     # 安装 / 更新 GrokTask MCP 条目
GrokTask agents mode claude mcp
GrokTask agents mode codex none    # 仅移除 GrokTask 条目
GrokTask agents mode claude none
```

配置编辑器只改动 GrokTask MCP 服务块；在文件合法时会保留其他 server 与注释。

#### 桌面 UI

| 平台 | 行为 |
| --- | --- |
| macOS | 菜单栏图标；左键锚定浮层，右键原生菜单。 |
| Windows / Linux | 系统托盘，交互模式相同（Linux 托盘宿主受限时可能回退，见 `doctor`）。 |

浮层与完整窗口共享同一任务时间线与展开状态。托盘可见性：`off` \| `active` \| `always`（仅 `always` 使用登录项）。默认 `off`，避免后台 MCP 任务强制拉起 GUI。

#### Doctor

```bash
GrokTask doctor
```

报告二进制路径、Grok 可用性、daemon/GUI host 状态、托盘能力与 Agent 集成状态。适合「MCP 正常但 Linux 上没有托盘」这类排查。

### 数据路径与安全

默认数据目录（测试/隔离可用 `GROKTASK_HOME` 覆盖）：

```text
~/.groktask/
  config.json
  history.sqlite3
  daemon.lock / daemon.json / daemon.sock
  gui-host.lock / gui-host.sock
  daemon.log
  gui.log
```

自动化测试请勿在未显式使用临时目录的情况下指向真实用户的 `~/.groktask`。

### 开发与验证

前端：

```bash
pnpm install --frozen-lockfile
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm build
```

Rust：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features
cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol
```

Tauri（不生成安装包）：

```bash
pnpm tauri build --no-bundle
```

仅前端门禁：`pnpm ci:frontend`。

完整产品验收标准（含手动 Grok 冒烟）见 [`docs/acceptance.md`](docs/acceptance.md)。真实 Grok E2E 为**手动 / 可选**，普通 CI 不要求。

| 文档 | 路径 |
| --- | --- |
| 规格 | [`docs/specs/`](docs/specs/) |
| 独立化重构计划 | [`docs/plans/standalone-refactor.md`](docs/plans/standalone-refactor.md) |
| Activity 应用迁移说明 | [`docs/grok-activity-app.md`](docs/grok-activity-app.md) |

### 发布状态

CI 在 push/PR 上跑质量门禁（前端 + 多 target Rust）。独立的 **release** 工作流（`workflow_dispatch` 与版本 tag）会为下列 target 构建**未签名**二进制：

| Target |
| --- |
| `aarch64-apple-darwin` |
| `x86_64-apple-darwin` |
| `x86_64-pc-windows-msvc` |
| `x86_64-unknown-linux-gnu` |

产物按 **target 与版本** 命名，并附带 **SHA-256** 校验文件。产物**未公证**，也**尚未**打包为各平台安装器——本仓库未接入 secrets、代码签名或应用商店发布。

目前可能**尚无已发布的 GitHub Release**；请使用 Actions 产物或从源码构建。详见 [`.github/workflows/release.yml`](.github/workflows/release.yml)。

### 从旧版 Codex 插件迁移

若你曾安装本仓库实验性 **Codex 插件**（skills、hooks、Node companion、localhost 活动页）：

1. 通过 Codex 正常流程**卸载/禁用旧插件**。
2. 若本地仍有旧的 `plugins/grok-codex` 检出，请**删除**（Phase 6 后本仓库不再包含该树）。
3. 通过 **设置 → Integrations** 或 `GrokTask agents mode codex mcp` / `claude mcp` **安装 GrokTask MCP**。
4. 尽量只保留**一套** Grok 集成：若旧插件 MCP 条目仍在，doctor/status 可能提示重复；未显式 remove 时 GrokTask 不会删除其他工具的配置。

旧 **localhost 看板** 已由原生浮层与完整窗口替代。见 [`docs/grok-activity-app.md`](docs/grok-activity-app.md)。

### 许可证

- **MIT** — 见 [`LICENSE`](LICENSE)。
- 对开源 ACP 客户端的设计借鉴仅为概念层面；本项目不复制 GPL 代码。
- 当前树内没有以需要额外版权声明的方式拷贝第三方源码；除 MIT 与 crates/npm 依赖自带许可证外，无额外版权声明要求。

---

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=xuqian-git/GrokTask&type=Date)](https://star-history.com/#xuqian-git/GrokTask&Date)
