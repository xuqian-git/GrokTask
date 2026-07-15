# Migration note: old localhost dashboard (removed)

**Status:** Historical only. The Codex plugin localhost dashboard was removed in Phase 6.

## What changed

Earlier iterations of this repository shipped a Codex plugin (`grok-codex`) with a Node companion and a secret **localhost** activity page. That architecture is gone.

GrokTask is now a standalone desktop application:

| Concern | Current approach |
| --- | --- |
| Agent integration | MCP server role: `GrokTask mcp` (tools: `run`, `start`, `continue`, `status`, `wait`, `cancel`) |
| Live Thought → Tool → Reply UI | Native Tauri WebView (menu-bar/tray popover and full window) |
| Install / remove agent config | Settings → Integrations, or `GrokTask agents mode codex\|claude mcp\|none` |
| Diagnostics | `GrokTask doctor` |

There is no localhost HTTP dashboard, no plugin MCP Apps resource, and no Node companion runtime in this repository.

## Where to go instead

- **Users:** see the root [README](../README.md) for install, MCP setup, CLI usage, and uninstall of the old plugin.
- **Specs:** [docs/specs/](specs/) describes the standalone product, ACP runtime, CLI/MCP contract, and integrations.
- **Acceptance:** [docs/acceptance.md](acceptance.md) is the delivery checklist.

Do not treat any pre-Phase-6 dashboard URLs, `grok_activity_*` tool names, or plugin skill paths as current documentation.
