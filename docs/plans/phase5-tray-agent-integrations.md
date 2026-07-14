# Phase 5 Tray, windows, settings, and Agent integrations brief

Status: ready to delegate to Grok after user approval. Codex owns this brief; Grok owns all product/test code.

This phase implements the native desktop shell and Agent integration management. It must not start Phase 6 migration, old plugin deletion, README rewrite, release packaging, signing, checksums, or CI release matrix work.

## Why this phase exists

Phase 4 made the conversation UI useful, but GrokTask is still not a native desktop tool in the AskHuman-like sense:

- no native tray/menu-bar entry;
- no anchored popover window separate from the main window;
- Settings is still a placeholder;
- Codex/Claude Code MCP integration status and install/update/remove are not implemented;
- CLI routing for `setup`, `app`, and `agents` is not complete.

Phase 5 turns the app into a first-class local desktop tool while keeping all external writes explicit, scoped, and testable.

Primary specs:

- `docs/specs/architecture.md`, section 7 and related app boundaries;
- `docs/specs/integrations.md`;
- `docs/specs/cli-mcp.md`, `doctor` / CLI routing portions;
- `docs/specs/persistence-ipc.md`, config / lease portions when directly needed;
- `docs/acceptance.md`, sections 11–12.

Reference app:

- `/Users/qian/project/AskHuman` is a read-only UX and architecture reference for native tray/menu-bar behavior, settings, and blocking-tool ergonomics.
- Do not modify AskHuman.
- Borrow product patterns only unless a file is explicitly licensed and attribution is preserved. Prefer implementing GrokTask-specific code from specs.

## Scope for this batch

### In scope

1. Native windows and tray/menu-bar shell
   - Add a native tray/menu-bar icon through Tauri 2.
   - Left-click toggles an anchored popover window.
   - Right-click native menu includes:
     - current task/status summary placeholder from existing task list/status APIs where available;
     - Open current task / Open GrokTask / History / Settings;
     - daemon status/restart entry if current daemon helpers support it;
     - Quit GrokTask.
   - Popover is a separate frameless/floating WebView window using `?view=popover`, not the main window pretending to be a popover.
   - Main/history/settings windows remain single-instance: repeated CLI/menu opens route/focus existing windows.
   - Implement monitor work-area clamping and coordinate fallback. macOS should place the popover below the menu bar icon when coordinates are available. Linux without reliable tray click coordinates must fall back to a deterministic top-right position and report degraded capability.

2. Tray lifecycle config
   - Use existing config `general.trayMode: off | active | always`.
   - Settings > General exposes tray mode.
   - Implement pure/platform-adapter boundaries for login items:
     - macOS LaunchAgent;
     - Windows current-user startup / registry adapter;
     - Linux XDG autostart `.desktop` adapter.
   - Tests may use temp directories / fake adapters only. Do not install a real login item during normal tests.
   - `always` is the only mode that creates/updates a login item. `off` and `active` remove the login item.

3. Agent integration management
   - Implement Codex user-level MCP integration:
     - target `~/.codex/config.toml`;
     - only `[mcp_servers.groktask]`;
     - `command = "<absolute current GrokTask path>"`;
     - `args = ["mcp"]`;
     - `startup_timeout_sec = 30`;
     - `tool_timeout_sec = 86400`.
   - Implement Claude Code user-level MCP integration:
     - target `~/.claude.json`;
     - only top-level `mcpServers.groktask`;
     - `command`, `args: ["mcp"]`, `timeout: 86400000`.
   - Status states:
     - `not_installed`;
     - `installed`;
     - `outdated`;
     - `invalid_config`;
     - `unavailable`.
   - Operations:
     - status;
     - install/update via idempotent upsert;
     - remove via no-op-safe delete.
   - Safety requirements:
     - minimal semantic edit;
     - preserve unrelated config entries;
     - do not overwrite invalid config;
     - atomic write in same directory;
     - preserve permissions when practical;
     - command path is data, not shell-escaped text;
     - tests use temp HOME/config roots and must never touch the real `~/.codex/config.toml` or `~/.claude.json`.

4. Settings UI
   - Replace placeholder `SettingsView.vue` with tabs/sections:
     - General: tray mode, language/theme placeholders if not fully wired, popover size, history limit summary;
     - Integrations: Codex and Claude cards with status, config file path, binary path, Install/Update/Remove actions, and post-change reminder;
     - Diagnostics: Grok CLI detection summary and daemon/tray capability status;
     - History: history limit / clear-history placeholder if backend clear is not in this phase.
   - UI actions must report expected impact before external config writes and refresh status after completion.
   - If a backend operation is not yet safe to expose, show disabled UI with a clear reason rather than a fake success.

5. CLI routing
   - Add / complete:
     - `GrokTask app`;
     - `GrokTask setup`;
     - `GrokTask agents status`;
     - `GrokTask agents status codex|claude`;
     - `GrokTask agents mode codex mcp|none`;
     - `GrokTask agents mode claude mcp|none`.
   - `setup` routes to the single Settings window / Integrations page; it must not silently modify config.
   - CLI output should have a JSON mode if the existing CLI pattern already supports it; otherwise keep text stable and tested.

6. Grok CLI detection in Settings / doctor
   - Display:
     - executable path;
     - version when available;
     - login/availability state if detectable without reading xAI tokens;
     - actionable install/login guidance.
   - Do not read or store xAI tokens.
   - Do not initiate interactive login automatically.

### Out of scope

- Deleting old plugin files, Node companion, localhost dashboard docs, old package scripts.
- README migration.
- Release CI matrix, signing, notarization, checksums, installers.
- Real E2E across every OS.
- Full lease/data-connection refactor unless strictly required for tray/window tests.
- Silent modification of user Agent configs during tests or review.
- Installing a real login item during tests.
- Removing or modifying AskHuman.

## Current code to start from

- `src-tauri/src/app/gui_host.rs`
- `src-tauri/src/app/mod.rs`
- `src-tauri/src/config.rs`
- `src-tauri/src/cli/mod.rs`
- `src-tauri/src/cli/help.rs`
- `src-tauri/src/ipc/protocol.rs`
- `src-tauri/src/ipc/transport.rs`
- `src-tauri/src/paths.rs`
- `src/views/SettingsView.vue`
- `src/App.vue`
- `src/views/PopoverView.vue`
- `src/lib/ipc.ts`
- `docs/specs/integrations.md`

Grok may add modules such as:

- `src-tauri/src/app/tray.rs`
- `src-tauri/src/app/windows.rs`
- `src-tauri/src/app/login_item.rs`
- `src-tauri/src/integrations/`
- `src-tauri/src/doctor/`
- frontend settings components under `src/components/settings/`

## Dependencies and implementation notes

- Tauri already has `tray-icon` enabled.
- For Codex TOML, prefer `toml_edit` or an equivalent minimal-edit approach. If adding a dependency, justify it in the final report.
- For Claude JSON, use a minimal CST-preserving approach. If exact whitespace preservation is too risky for this batch, preserve unrelated semantic content and avoid rewriting on invalid config; document the limitation clearly and cover it with tests.
- Keep platform-specific code behind small adapters and `cfg(...)` gates so current macOS and target Windows/Linux compile.
- If a platform API cannot be fully tested in CI, expose a pure calculation/helper and fake adapter tests.
- Any Tauri command that can write external config should validate target and return structured errors; frontend should not guess.

## Required tests

Add deterministic automated coverage for:

1. Codex TOML integration
   - not installed → install → installed;
   - outdated path/timeout → update → installed;
   - remove only `groktask`;
   - unrelated servers, comments, and key order preserved as much as the chosen editor supports;
   - invalid TOML does not write.

2. Claude JSON integration
   - not installed → install → installed;
   - outdated path/timeout → update → installed;
   - remove only `groktask`;
   - unrelated servers and top-level keys preserved semantically;
   - invalid JSON / wrong parent type does not write.

3. Tray/window helpers
   - popover position clamps to monitor work area;
   - Linux no-coordinate fallback is deterministic;
   - tray mode `off|active|always` maps to the correct login-item operation;
   - right-click menu model updates from task/daemon summary inputs.

4. CLI
   - `agents status` output is stable;
   - `agents mode ... mcp|none` uses the same integration engine as Settings;
   - `setup` routes to Settings and performs no config writes.

5. Frontend Settings
   - Integration cards show status/path/binary/action buttons;
   - General tray mode controls reflect current config;
   - action result refreshes displayed status;
   - invalid/unavailable state disables destructive action or shows clear error.

6. Regression gates
   - Phase 4 frontend tests still pass.
   - Existing daemon/CLI/MCP tests still pass.

## Acceptance commands for this batch

Run at minimum:

```text
pnpm lint
pnpm test
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --all-features
```

If platform-specific code is added, also run any available target compile checks that are already configured in the project.

## Reporting requirements

At the end, Grok must report:

- changed files;
- which Phase 5 items were fully implemented;
- any platform behavior intentionally degraded and why;
- tests run and exact results;
- known gaps left for Phase 6 or later;
- confirmation that it did not delete old plugin files, rewrite README, create release artifacts, push, or commit.
