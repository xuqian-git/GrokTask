# GrokTask

Cross-platform local tool that lets Codex / Claude Code hand coding and review work to Grok Build via MCP, while you watch the real Thought → Tool → Reply stream in a native desktop UI.

**Status:** Phase 0–1 foundations (skeleton, config, SQLite, IPC, daemon lifecycle). Conversation UI, full MCP tools, and tray land in later phases. The legacy `plugins/grok-codex` tree remains until Phase 6.

## Stack

- Tauri 2 + Rust (Tokio) + Vue 3 + TypeScript + Vite
- Single `GrokTask` binary with role dispatch: CLI, `mcp`, `daemon`, `--gui-host`, `--task-supervisor`
- Bundled SQLite (WAL), NDJSON local IPC (Unix socket / Windows named pipe)

## Development

```bash
pnpm install
pnpm build
pnpm lint
pnpm typecheck
pnpm test

cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --all-features
cargo build --manifest-path src-tauri/Cargo.toml --release --features custom-protocol

# Tauri no-bundle (embeds frontend; does not produce installers)
pnpm tauri build --no-bundle
```

Production builds **must** enable `--features custom-protocol` so frontend assets are embedded (otherwise the window is blank). Prefer `pnpm tauri build --no-bundle` for a full Tauri release compile without packaging; the double-dash form `pnpm tauri build -- --no-bundle` is incorrect for this CLI.

### Roles (no accidental GUI)

```bash
./src-tauri/target/release/GrokTask --help
./src-tauri/target/release/GrokTask --version
./src-tauri/target/release/GrokTask mcp          # does not start Tauri
./src-tauri/target/release/GrokTask daemon run   # no WebView
./src-tauri/target/release/GrokTask daemon status
```

Hidden internal roles: `--gui-host`, `--task-supervisor`.

### Data directory

```text
~/.groktask/
  config.json
  history.sqlite3
  daemon.lock / daemon.json / daemon.sock
  gui-host.lock / gui-host.sock
  daemon.log
```

Override with `GROKTASK_HOME` for tests/isolation.

## Specs

See `docs/specs/` and `docs/plans/standalone-refactor.md`.

## License

MIT — see `LICENSE`.
