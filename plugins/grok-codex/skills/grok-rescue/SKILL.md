---
name: grok-rescue
description: Delegate a substantial coding, debugging, diagnosis, refactoring, or second-implementation task from Codex to Grok Build with a live local ACP activity dashboard when available. Use when the user explicitly asks to use Grok, wants Grok progress, or when an independent coding-agent pass is valuable. Do not use for trivial work. Grok may read repository content and, unless --read is selected, modify the workspace through an external xAI service.
---

# Grok rescue

Route the task to Grok Build. Do not solve the delegated task yourself.

## Prepare the task

Preserve the user's intent and include only information Grok needs for a
one-shot run:

- the desired outcome;
- relevant files, directories, errors, or constraints;
- acceptance criteria and verification commands;
- whether the result should include edits or only analysis.

Keep routing controls separate from task text. Supported controls are
`--read`, `--write`, `--wait`, `--background`, `--effort <level>`, `--model
<id>`, `--cwd <path>`, `--best-of-n <N>`, `--check`, `--worktree`,
`--worktree=<name>`, and `--resume`.

Defaults:

- Use write-capable mode unless the user requests review, research, diagnosis,
  planning, or no edits; use `--read` for those cases.
- Wait for a small, bounded run. Use `--background` for long, open-ended, or
  multi-step work.
- Leave model and effort unset unless the user specifies them.

## Start live activity

Prefer the plugin MCP tools when `grok_activity_start` is available:

1. Call `grok_activity_start` exactly once. Pass the prepared prompt, the
   current workspace as an absolute `cwd`, and `mode: "read"` or `"write"`.
   Forward `model`, `effort`, and `check` only when requested.
2. Read the returned `dashboardUrl`. When the Codex in-app Browser capability
   is available, open or reuse this localhost URL there immediately. Do not
   launch Chrome or another external browser. The page follows the latest job
   and displays the full local ACP activity stream, including raw thought and
   tool payloads. When
   `browser:control-in-app-browser` is listed, load and follow that skill for
   the navigation.
3. Unless the user requested background execution, call `grok_activity_wait`
   with the returned `jobId`. Repeat bounded waits while the status remains
   active and the task is still making progress.
4. Review Grok's public result and workspace changes before claiming success.
   Report failures as failures; do not replace them with a Codex-generated
   implementation.

If the user requests `--best-of-n`, `--worktree`, or `--resume`, use the
companion because those routes are not represented by the activity MCP tool.
Use the companion as a fallback when the MCP tools are unavailable in the
current host.

## Companion fallback

Resolve `../../scripts/grok-companion.mjs` relative to this `SKILL.md`. Invoke
it once, safely quoting every argument and never using `eval`:

```bash
node "<resolved-plugin-root>/scripts/grok-companion.mjs" task <routing-flags> -- "<task>"
```

Return companion output without substitute analysis. If invocation fails,
report the failure and direct the user to `$grok-setup`.
