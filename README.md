# GrokTask

Cross-platform local task runner that lets **Codex** or **Claude Code** hand planned coding implementation work to **Grok Build** via MCP, while you watch the real Thought → Tool → Reply stream in a native desktop UI (menu bar / tray popover and full window).

GrokTask is a **standalone** single binary (`GrokTask`), not a Codex plugin. It does not depend on a browser, localhost dashboard, or Node.js plugin runtime.

## What it solves

- **Blocking agent calls** that wait until Grok finishes (AskHuman-like: the MCP/CLI `run` call blocks until the turn completes).
- **Async control** with `start` / `status` / `wait` / `cancel` that work across process restarts via a local daemon.
- **Explicit safety modes**: every task must pass `mode: read` or `mode: write` (no defaults, no text inference).
- **Native visibility**: live timeline, plan bar, history, and settings without Chrome or an in-app browser page.

## Prerequisites

### Runtime (end users)

- **Grok CLI / Grok Build** installed and authenticated (`grok login` or equivalent official flow).
- A platform desktop session if you want the tray / menu-bar UI (MCP and CLI work without a GUI).

### Development

| Tool | Notes |
| --- | --- |
| Node.js | ≥ 20 |
| pnpm | 9.x (see `packageManager` in `package.json`) |
| Rust | stable, matching `rust-version` in `src-tauri/Cargo.toml` |
| Platform deps | Linux needs WebKitGTK and related packages for Tauri (see CI workflow) |

## Install and build

```bash
# Frontend deps
pnpm install --frozen-lockfile

# Local frontend build (Vite output under dist/)
pnpm build

# Release Rust binary with embedded frontend assets (required for non-blank windows)
cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol

# Full Tauri compile without producing OS installers
pnpm tauri build --no-bundle
```

Production builds **must** enable `--features custom-protocol` so frontend assets are embedded. Prefer `pnpm tauri build --no-bundle` for a full Tauri release compile without packaging. The form `pnpm tauri build -- --no-bundle` is incorrect for this CLI.

Binary path after cargo release build:

```text
src-tauri/target/release/GrokTask
# or, when building for a specific target:
src-tauri/target/<triple>/release/GrokTask
```

## Usage

### Roles (same binary)

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

### CLI tasks

Mode is always explicit (`read` or `write`). There is no default.

```bash
# Blocking: waits until Grok returns a final/partial/cancelled/failed result
GrokTask run --mode read --cwd /absolute/path "Summarize this repo"

# Async start then poll / wait
GrokTask start --mode write --cwd /absolute/path --submission-id <uuid> "Apply the fix"
GrokTask status <taskId>
GrokTask wait <taskId> <turnId> [--timeout SECONDS]
GrokTask cancel <taskId> --turn <turnId>
```

- **`run` blocks** until the turn finishes. Callers (MCP or shell) should treat this like a long-running tool invocation.
- **`read`**: read-only sandbox expectations; workspace should not be modified.
- **`write`**: Grok may edit files under the given `cwd`. GrokTask itself does not auto-commit, push, or open PRs. Sandbox limits are not an unbreakable security boundary; treat write mode as trusted local automation.

### MCP (Codex / Claude)

```bash
GrokTask mcp
```

Server name: `groktask`. Tools only: `run`, `start`, `status`, `wait`, `cancel`. No UI resource, no localhost URL, no MCP Apps template.

Install agent config from:

- **Settings → Integrations** (install / remove for Codex or Claude), or
- CLI:

```bash
GrokTask agents status
GrokTask agents mode codex mcp     # install / update GrokTask MCP entry
GrokTask agents mode claude mcp
GrokTask agents mode codex none    # remove GrokTask entry only
GrokTask agents mode claude none
```

Config editors only touch the GrokTask MCP server block; other servers and comments are preserved when the file is valid.

### Desktop UI

- **macOS**: menu bar icon; left-click opens an anchored popover; right-click opens a native menu.
- **Windows / Linux**: system tray with the same left-click / right-click pattern (Linux may fall back when tray hosts are limited; see `doctor`).
- Popover and full window share the same task timeline and expansion state.
- Tray visibility modes: `off` | `active` | `always` (login item only for `always`). Default is `off` so background MCP work does not force a GUI up.

### Doctor

```bash
GrokTask doctor
```

Reports binary paths, Grok availability, daemon/GUI host state, tray capability, and agent integration status. Useful when MCP works but the tray is missing on Linux.

## Data paths and config

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

## Development and verification

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

Compose frontend gates only:

```bash
pnpm ci:frontend
```

Full product acceptance criteria (including manual Grok smoke tests) live in [`docs/acceptance.md`](docs/acceptance.md). Real Grok E2E is **manual / opt-in** and is not required for ordinary CI.

Specs: [`docs/specs/`](docs/specs/). Refactor plan: [`docs/plans/standalone-refactor.md`](docs/plans/standalone-refactor.md).

## Migration from the old Codex plugin

If you previously installed the experimental **Codex plugin** from this repository (skills, hooks, Node companion, localhost activity page):

1. **Uninstall the old plugin** through Codex’s normal plugin uninstall / disable flow so Codex no longer loads it.
2. **Remove any leftover local plugin checkout** if you still have an old `plugins/grok-codex` copy on disk (this repo no longer ships that tree after Phase 6).
3. **Install GrokTask MCP** via Settings → Integrations or `GrokTask agents mode codex mcp` / `claude mcp`.
4. Prefer a **single** Grok integration: if an old plugin MCP entry remains, doctor/status may warn about duplicates; GrokTask will not delete other tools’ config without an explicit remove.

The old **localhost dashboard** is replaced by the native popover and full window. See [`docs/grok-activity-app.md`](docs/grok-activity-app.md) for a short migration note (not an active runtime guide).

## Release artifacts and checksums

CI quality gates run on push/PR (frontend + multi-target Rust). A separate **release** workflow (workflow_dispatch and version tags) builds unsigned binaries for:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-pc-windows-msvc`
- `x86_64-unknown-linux-gnu`

Each artifact is named with **target and version**, and is uploaded together with a **SHA-256** checksum file. Artifacts are **unsigned** and are **not** notarized or wrapped as platform installers yet—no secrets, code signing, or store publishing are wired in this repository.

See [`.github/workflows/release.yml`](.github/workflows/release.yml).

## License and attribution

- License: **MIT** — see [`LICENSE`](LICENSE).
- Design inspiration from open-source ACP clients is conceptual only; this project does not copy GPL code.
- No third-party source was copied into this tree in a way that currently requires additional copyright notices beyond the MIT license and normal dependency licenses from crates/npm packages.
