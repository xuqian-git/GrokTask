---
name: grok-cli-runtime
description: Operate the bundled grok-codex companion runtime for Grok Build setup, task delegation, working-tree review, and branch review. Use when another Grok skill needs exact invocation semantics, routing flags, foreground/background behavior, or failure handling.
---

# Grok CLI runtime

Use the single companion at `../../scripts/grok-companion.mjs`, resolved from
this `SKILL.md` rather than from the user's working directory.

```text
node <companion> setup [--json]
node <companion> task [flags] -- <task>
node <companion> review [flags] -- <focus>
node <companion> adversarial-review [flags] -- <focus>
```

The runtime discovers `GROK_BIN` first, then `grok` on `PATH`, then the default
user install path. It recognizes either `XAI_API_KEY` or a non-empty cached
Grok login file.

Task runs are write-capable by default and pass `--always-approve` to Grok.
`--read` selects plan mode. Reviews always use plan mode, disable web search,
and receive only a captured Git diff. Background runs detach and return a PID
plus a log path containing Grok's raw JSON output.

Always pass user text after `--`, quote arguments safely, and never use `eval`.
On failure, return the companion's diagnostic instead of replacing Grok with a
Codex-generated result.
