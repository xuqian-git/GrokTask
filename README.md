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

GrokTask is a **standalone, cross-platform local task runner**. One binary provides CLI, MCP, a local daemon, and a native Tauri desktop UI (menu bar / tray popover and full window).

| You keep… | GrokTask handles… |
| --- | --- |
| Planning, review, diagnosis (Codex / Claude Code) | Implementation via Grok Build MCP |
| Explicit `read` / `write` on every task | Blocking `run` or async `start` / `status` / `wait` / `cancel` |
| Final judgment on the workspace | Live Thought → Tool → Reply timeline |

**Key capabilities:** `run` blocks until the turn finishes; async control survives process restarts via the local daemon; every task requires `mode: read` or `mode: write` (no defaults, no text inference); native timeline, plan bar, history, and settings.

### Prerequisites

- **Runtime:** [Grok CLI / Grok Build](https://docs.x.ai) installed and authenticated (`grok login` or official auth). Tray UI needs a desktop session; MCP/CLI work headless.
- **Development:** Node.js ≥ 20 · pnpm 9.x (`packageManager` in `package.json`) · Rust stable (`rust-version` in `src-tauri/Cargo.toml`) · Linux: WebKitGTK and related deps ([CI](.github/workflows/ci.yml)).

### Build from source

No package-manager install is published yet:

```bash
pnpm install --frozen-lockfile
pnpm build
cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol
# full Tauri compile without OS installers:
pnpm tauri build --no-bundle
```

```text
src-tauri/target/release/GrokTask
# or: src-tauri/target/<triple>/release/GrokTask
```

Production builds **must** use `--features custom-protocol` so assets are embedded. Prefer `pnpm tauri build --no-bundle` (not `pnpm tauri build -- --no-bundle`).

### Usage

#### Roles (same binary)

```bash
GrokTask --help
GrokTask --version
GrokTask mcp                 # stdio MCP; no Tauri
GrokTask daemon run          # no WebView
GrokTask daemon status
GrokTask doctor
GrokTask app                 # desktop UI / ensure GUI host
```

Hidden roles: `--gui-host`, `--task-supervisor` (not everyday use).

#### CLI tasks

Mode is always explicit (`read` or `write`) — no default.

```bash
# Blocking: waits until final / partial / cancelled / failed
GrokTask run --mode read --cwd /absolute/path "Summarize this repo"

# Async
GrokTask start --mode write --cwd /absolute/path --submission-id <uuid> "Apply the fix"
GrokTask status <taskId>
GrokTask wait <taskId> <turnId> [--timeout SECONDS]
GrokTask cancel <taskId> --turn <turnId>
```

| Mode / command | Behavior |
| --- | --- |
| `run` | Blocks until the turn finishes (long-running tool call). |
| `read` | Read-only expectations; workspace should not be modified. |
| `write` | Grok may edit files under `cwd`. GrokTask does not auto-commit, push, or open PRs. Sandbox limits are not an unbreakable security boundary — treat write as trusted local automation. |

#### MCP (Codex / Claude)

```bash
GrokTask mcp   # server: groktask · tools: run, start, status, wait, cancel
```

Install from **Settings → Integrations**, or:

```bash
GrokTask agents status
GrokTask agents mode codex mcp     # install / update
GrokTask agents mode claude mcp
GrokTask agents mode codex none    # remove GrokTask entry only
GrokTask agents mode claude none
```

Editors only touch the GrokTask MCP block; other servers and comments stay when the file is valid.

#### Desktop UI & doctor

| Platform | Behavior |
| --- | --- |
| macOS | Menu bar; left-click popover; right-click menu. |
| Windows / Linux | System tray (Linux may fall back; see `doctor`). |

Popover and full window share the task timeline. Tray visibility: `off` \| `active` \| `always` (login item only for `always`). Default `off`.

```bash
GrokTask doctor   # paths, Grok, daemon/GUI host, tray, agent integration
```

### Data paths and safety

Default home (override with `GROKTASK_HOME`):

```text
~/.groktask/   # config.json, history.sqlite3, daemon.*, gui-host.*, daemon.log, gui.log
```

Do not point automated tests at a real user `~/.groktask` without an explicit temp home.

### Development and verification

```bash
# Frontend
pnpm install --frozen-lockfile
pnpm format:check && pnpm lint && pnpm typecheck && pnpm test && pnpm build

# Rust
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features
cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol

# Tauri (no installer packages)
pnpm tauri build --no-bundle
```

Frontend gates only: `pnpm ci:frontend`. Acceptance (optional manual Grok smoke): [`docs/acceptance.md`](docs/acceptance.md). Real Grok E2E is **manual / opt-in**, not required for ordinary CI.

Specs: [`docs/specs/`](docs/specs/) · Standalone refactor plan: [`docs/plans/standalone-refactor.md`](docs/plans/standalone-refactor.md)

### Release status

CI quality gates run on push/PR. A separate **release** workflow (`workflow_dispatch` and version tags) builds **unsigned** binaries for `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`, and `x86_64-unknown-linux-gnu`.

Artifacts are named with **target and version** and include **SHA-256** checksums. They are **not notarized** and are **not** platform installers — no secrets, code signing, or store publishing are wired here. There may be **no published GitHub Releases** yet; use Actions artifacts or build from source. See [`.github/workflows/release.yml`](.github/workflows/release.yml).

### License

**MIT** — see [`LICENSE`](LICENSE). Design inspiration from open-source ACP clients is conceptual only; this project does not copy GPL code.

---

## 简体中文

### 它是什么

GrokTask 是一个**独立、跨平台的本地任务运行器**。同一个二进制提供 CLI、MCP、本地 daemon，以及原生 Tauri 桌面界面（菜单栏 / 托盘浮层与完整窗口）。

| 你继续负责… | GrokTask 负责… |
| --- | --- |
| 规划、审查、诊断（Codex / Claude Code） | 通过 MCP 把实现交给 Grok Build |
| 每次任务显式 `read` / `write` | 阻塞式 `run`，或异步 `start` / `status` / `wait` / `cancel` |
| 对工作区结果做最终判断 | 原生 Thought → Tool → Reply 时间线 |

**关键能力：** `run` 阻塞到本轮结束；异步控制经本地 daemon 跨进程重启仍可用；每次任务必须 `mode: read` 或 `mode: write`（无默认值、不从文案推断）；原生时间线、计划条、历史与设置。

### 前置条件

- **运行时：** 已安装并认证的 [Grok CLI / Grok Build](https://docs.x.ai)（`grok login` 或官方流程）。托盘 UI 需要桌面会话；MCP/CLI 可无 GUI。
- **开发环境：** Node.js ≥ 20 · pnpm 9.x（`package.json` 的 `packageManager`） · Rust stable（`src-tauri/Cargo.toml` 的 `rust-version`） · Linux 需 WebKitGTK 等（见 [CI](.github/workflows/ci.yml)）。

### 从源码构建

目前尚无包管理器上的正式安装发布：

```bash
pnpm install --frozen-lockfile
pnpm build
cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol
# 完整 Tauri 编译且不打系统安装包：
pnpm tauri build --no-bundle
```

```text
src-tauri/target/release/GrokTask
# 或：src-tauri/target/<triple>/release/GrokTask
```

生产构建**必须**启用 `--features custom-protocol`。请用 `pnpm tauri build --no-bundle`（不要用 `pnpm tauri build -- --no-bundle`）。

### 使用说明

#### 同一二进制的多种角色

```bash
GrokTask --help
GrokTask --version
GrokTask mcp                 # stdio MCP；不启动 Tauri
GrokTask daemon run          # 无 WebView
GrokTask daemon status
GrokTask doctor
GrokTask app                 # 桌面 UI / 确保 GUI host
```

内部隐藏角色：`--gui-host`、`--task-supervisor`（日常无需使用）。

#### CLI 任务

模式始终显式（`read` 或 `write`），没有默认值。

```bash
# 阻塞：直到 final / partial / cancelled / failed
GrokTask run --mode read --cwd /absolute/path "Summarize this repo"

# 异步
GrokTask start --mode write --cwd /absolute/path --submission-id <uuid> "Apply the fix"
GrokTask status <taskId>
GrokTask wait <taskId> <turnId> [--timeout SECONDS]
GrokTask cancel <taskId> --turn <turnId>
```

| 模式 / 命令 | 行为 |
| --- | --- |
| `run` | 阻塞直到本轮结束（长耗时工具调用）。 |
| `read` | 只读预期，不应修改工作区。 |
| `write` | Grok 可在 `cwd` 下改文件。GrokTask **不会**自动 commit、push 或开 PR。沙箱不是牢不可破的安全边界——请将 write 视为受信任的本机自动化。 |

#### MCP（Codex / Claude）

```bash
GrokTask mcp   # 服务名：groktask · 工具：run, start, status, wait, cancel
```

通过 **设置 → Integrations** 或 CLI：

```bash
GrokTask agents status
GrokTask agents mode codex mcp     # 安装 / 更新
GrokTask agents mode claude mcp
GrokTask agents mode codex none    # 仅移除 GrokTask 条目
GrokTask agents mode claude none
```

配置编辑器只改动 GrokTask MCP 块；文件合法时保留其他 server 与注释。

#### 桌面 UI 与 Doctor

| 平台 | 行为 |
| --- | --- |
| macOS | 菜单栏；左键浮层，右键菜单。 |
| Windows / Linux | 系统托盘（Linux 可能回退，见 `doctor`）。 |

浮层与完整窗口共享任务时间线。托盘可见性：`off` \| `active` \| `always`（仅 `always` 使用登录项）。默认 `off`。

```bash
GrokTask doctor   # 路径、Grok、daemon/GUI host、托盘、Agent 集成
```

### 数据路径与安全

默认数据目录（可用 `GROKTASK_HOME` 覆盖）：

```text
~/.groktask/   # config.json, history.sqlite3, daemon.*, gui-host.*, daemon.log, gui.log
```

自动化测试请勿在未使用临时目录时指向真实用户的 `~/.groktask`。

### 开发与验证

```bash
# 前端
pnpm install --frozen-lockfile
pnpm format:check && pnpm lint && pnpm typecheck && pnpm test && pnpm build

# Rust
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features
cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol

# Tauri（不生成安装包）
pnpm tauri build --no-bundle
```

仅前端门禁：`pnpm ci:frontend`。验收（可选手动 Grok 冒烟）：[`docs/acceptance.md`](docs/acceptance.md)。真实 Grok E2E 为**手动 / 可选**，普通 CI 不要求。

规格：[`docs/specs/`](docs/specs/) · 独立化重构计划：[`docs/plans/standalone-refactor.md`](docs/plans/standalone-refactor.md)

### 发布状态

CI 在 push/PR 上跑质量门禁。独立的 **release** 工作流（`workflow_dispatch` 与版本 tag）为 `aarch64-apple-darwin`、`x86_64-apple-darwin`、`x86_64-pc-windows-msvc`、`x86_64-unknown-linux-gnu` 构建**未签名**二进制。

产物按 **target 与版本** 命名，附带 **SHA-256** 校验。产物**未公证**，也**尚未**打包为各平台安装器——本仓库未接入 secrets、代码签名或应用商店发布。目前可能**尚无已发布的 GitHub Release**；请使用 Actions 产物或从源码构建。详见 [`.github/workflows/release.yml`](.github/workflows/release.yml)。

### 许可证

**MIT** — 见 [`LICENSE`](LICENSE)。对开源 ACP 客户端的设计借鉴仅为概念层面；本项目不复制 GPL 代码。

---

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=xuqian-git/GrokTask&type=Date)](https://star-history.com/#xuqian-git/GrokTask&Date)
