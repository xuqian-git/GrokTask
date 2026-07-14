---
name: grok-rescue
description: Delegate a substantial coding, debugging, diagnosis, refactoring, or second-implementation task from Codex to Grok Build. Use when the user explicitly asks to use Grok, or proactively when Codex is stuck and an independent coding-agent pass is valuable. Do not use for trivial work. Grok may read repository content and, unless --read is selected, modify the workspace through an external xAI service.
---

# Grok rescue

Act as a thin router to Grok Build. Do not solve the delegated task yourself.

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

## Invoke the companion

Resolve `../../scripts/grok-companion.mjs` relative to this `SKILL.md`. Invoke
it once, safely quoting every argument and never using `eval`:

```bash
node "<resolved-plugin-root>/scripts/grok-companion.mjs" task <routing-flags> -- "<task>"
```

Return the companion output without adding substitute analysis. If invocation
fails, report the failure and direct the user to `$grok-setup`; do not silently
complete the task in Codex instead.
