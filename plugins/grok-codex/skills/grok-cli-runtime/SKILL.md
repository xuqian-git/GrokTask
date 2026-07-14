---
name: grok-cli-runtime
description: Operate the grok-codex live activity MCP tools and bundled companion runtime for Grok Build setup, task delegation, working-tree review, and branch review. Use when another Grok skill needs exact local-dashboard routing, invocation semantics, foreground/background behavior, or failure handling.
---

# Grok CLI runtime

Use the activity MCP path for ordinary delegated tasks. Call
`grok_activity_start` with an absolute workspace `cwd`, then call
`grok_activity_wait` unless background execution was requested. The start
result includes a secret localhost `dashboardUrl`. When the Codex in-app
Browser capability is available, open or reuse that URL there immediately;
never launch Chrome or another external browser. The dashboard polls the local
status API and can cancel the active job. When the
`browser:control-in-app-browser` skill is listed, load and follow it for this
navigation.

Use the companion at `../../scripts/grok-companion.mjs` for setup, reviews,
unsupported task flags, or hosts without the activity MCP tools. Resolve it
from this `SKILL.md`, never from the user's working directory.

```text
node <companion> setup [--json]
node <companion> task [flags] -- <task>
node <companion> review [flags] -- <focus>
node <companion> adversarial-review [flags] -- <focus>
```

The runtime discovers `GROK_BIN` first, then `grok` on `PATH`, then the default
user install path. It recognizes either `XAI_API_KEY` or a non-empty cached
Grok login file.

Task runs are write-capable by default. Both Activity modes use `grok agent
stdio` and the ACP JSON-RPC flow. Activity `mode: "write"` and companion task
runs pass automatic tool approval to Grok. Activity `mode: "read"` uses
headless `dontAsk` permissions plus Grok's `read-only` sandbox and explicit
read-only tool rules, so it does not wait for TUI plan approval. Companion
`--read` and reviews use plan mode; reviews disable web search and receive only
a captured Git diff. Companion background runs detach and return a PID plus a
log path containing Grok's raw JSON output.

The Activity dashboard may surface all ACP events, including raw thought chunks
and tool payloads. Always pass companion user text after `--`, quote arguments
safely, and never use `eval`. On failure,
return the tool or companion diagnostic instead of replacing Grok with a
Codex-generated result.
