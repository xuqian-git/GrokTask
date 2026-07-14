# Phase 6 migration, documentation, and release brief

This phase finishes the standalone GrokTask refactor. It must remove the old Codex plugin architecture, make the repository documentation match the actual standalone app, and add release/verification scaffolding.

Do not implement new product behavior beyond migration/release polish unless a failing acceptance item requires a small fix. Do not call another coding agent. Do not commit or push.

## Context

Completed batches:

- Phase 0–1: standalone skeleton, config, SQLite, IPC, daemon lifecycle.
- Phase 2–3: ACP client, reducer, daemon, CLI, MCP tools.
- Phase 4: conversation UI.
- Phase 5: tray/menu-bar shell, settings, doctor, Codex/Claude integrations.

Remaining old plugin assets are intentionally still present and must be removed or converted now:

- `plugins/grok-codex/**`
- `test/*.test.mjs`
- `docs/grok-activity-app.md` still describes the old localhost dashboard/plugin runtime.
- README status still says Phase 0–1 and old plugin remains until Phase 6.
- `package.json` still has `test:legacy`.

## Goals

1. The repository is clearly a standalone GrokTask app, not a Codex plugin repo.
2. End users can install, configure, run, test, and uninstall/migrate from the old plugin using README instructions.
3. CI has release-oriented cross-platform build artifacts and SHA-256 checksums.
4. The automatic acceptance commands in `docs/acceptance.md` have matching package scripts or documented equivalents.
5. No current docs describe localhost dashboard/plugin MCP tools as the active architecture.

## Required implementation

### 1. Remove legacy plugin architecture

Delete:

- `plugins/grok-codex/**`
- Node companion/dashboard scripts under that plugin
- plugin skills/hooks/manifests
- legacy Node tests under `test/*.test.mjs`
- `test:legacy` from `package.json`

After deletion, repo-wide search must not find active references to:

- `.codex-plugin`
- `grok_activity_start`
- `grok-codex`
- `grok-activity-server.mjs`
- `grok-companion.mjs`
- `localhost dashboard`

Historical migration notes may mention the old plugin by name only when clearly describing removal/migration.

### 2. Rewrite README for standalone GrokTask

Replace the phase-status README with a user-facing standalone README covering:

- What GrokTask is and what problems it solves.
- Prerequisites:
  - Grok CLI / Grok Build installed and authenticated.
  - Node/pnpm/Rust requirements for development.
- Install/build:
  - local dev build
  - release build with `custom-protocol`
  - Tauri no-bundle build
- Usage:
  - CLI `run/start/status/wait/cancel`
  - blocking `run` semantics, especially “blocks until Grok returns,” matching AskHuman-like tool behavior.
  - explicit read/write modes and safety expectations.
  - MCP server command for Codex/Claude.
  - Settings → Integrations install/remove flow.
  - menu-bar/tray/popup/full window behavior.
  - doctor diagnostics.
- Data paths and config:
  - `~/.groktask`
  - `GROKTASK_HOME`
  - config/history/log locations.
- Development/test commands:
  - `pnpm install --frozen-lockfile`
  - `pnpm format:check`
  - `pnpm lint`
  - `pnpm typecheck`
  - `pnpm test`
  - `pnpm build`
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings`
  - `cargo test --manifest-path src-tauri/Cargo.toml --all-features`
  - `cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol`
  - `pnpm tauri build --no-bundle`
- Migration/uninstall from old plugin:
  - remove the old Codex plugin installation
  - remove old local plugin directory if present
  - use Settings → Integrations or `GrokTask agents install/remove` for new MCP config
  - note that old localhost dashboard is replaced by native UI.
- Release artifacts and checksums.
- License and attribution policy.

README must not claim features that do not exist. If an acceptance item remains manual/opt-in (for example real Grok smoke tests), label it as manual.

### 3. Convert old dashboard doc to migration note

Replace `docs/grok-activity-app.md` with a short migration note:

- It should state that the old Codex plugin localhost dashboard was removed in Phase 6.
- It should point to native GrokTask UI, MCP tools, and Settings integrations.
- It should not document old MCP tool descriptors, HTTP routes, or old active runtime behavior as current.

### 4. Release / CI scaffolding

Keep existing CI quality gates and add release-oriented artifact/checksum coverage.

Minimum acceptable implementation:

- Existing cross-platform Rust matrix remains.
- Artifacts are named with target and version or at least target.
- SHA-256 checksum files are generated and uploaded with each built binary/artifact.
- If a new `release.yml` is added, it should support `workflow_dispatch` and/or version tags and run at least:
  - frontend build
  - `cargo build --release --features custom-protocol` for macOS arm64/x86_64, Windows x86_64, Linux x86_64
  - checksum generation
  - artifact upload
- If release packaging/signing is not implemented, document it honestly as unsigned/no-installer artifacts for now.

Do not introduce secrets, notarization, code signing, or publishing without explicit user approval.

### 5. Acceptance scripts and docs consistency

Make `package.json` scripts line up with `docs/acceptance.md`:

- Ensure `format:check`, `lint`, `typecheck`, `test`, `build` exist and pass.
- Remove legacy script(s) that refer to deleted old plugin tests.
- If adding a top-level `ci` script, it may compose frontend checks only; Rust checks can stay documented in README/CI.

Update docs/spec references when they still describe the old plugin as current. Prefer small edits over broad rewrites.

### 6. License attribution

Keep `LICENSE` intact.

Add or update a short attribution section if necessary:

- Design inspiration from open-source ACP clients is conceptual.
- Do not copy GPL code.
- If any MIT/Apache code was copied materially, add the relevant notice.

If no third-party code was copied, say that no additional notices are currently required.

## Out of scope

- No new product features.
- No OAuth/auth integration.
- No notarization/signing/secrets.
- No real PR publishing.
- No real user config mutation in tests.
- No Phase 7 planning.

## Required verification

Run and report exact results:

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

If `pnpm install --frozen-lockfile` is already satisfied and skipped for time, say so explicitly and leave it for Codex to run.

Also run these repo hygiene checks:

```text
rg -n "\.codex-plugin|grok_activity_start|grok-activity-server|grok-companion|grok-plugin-codex" .
git status --short
```

Expected search result: no active old-runtime references. Migration notes may mention old names only in past-tense context.

## Grok final report requirements

Report:

- Changed/deleted files.
- Verification commands and exact results.
- Any docs that still intentionally mention the old plugin as migration history.
- Release artifact/checksum behavior.
- Known gaps or manual acceptance items left for Codex/user review.
