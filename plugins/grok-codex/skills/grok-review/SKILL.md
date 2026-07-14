---
name: grok-review
description: Run a read-only Grok Build code review of the current Git working tree or a branch relative to a base ref. Use when the user asks Grok to review changes, requests an independent review pass, or wants findings without fixes. Never edit files as part of this skill.
---

# Grok review

Run a review and return findings; do not apply fixes.

Resolve `../../scripts/grok-companion.mjs` relative to this `SKILL.md`. Invoke:

```bash
node "<resolved-plugin-root>/scripts/grok-companion.mjs" review <flags> -- "<optional focus>"
```

Supported flags:

- `--base <ref>` reviews the current branch relative to the base and implies
  branch scope.
- `--scope auto|working-tree|branch` selects the Git comparison.
- `--wait` and `--background` choose execution mode.
- `--effort <level>`, `--model <id>`, and `--cwd <path>` route the run.

Honor an explicit execution mode. Otherwise wait for roughly one or two small
files and use background mode for larger or uncertain reviews.

The companion captures the Git diff and forces Grok plan mode. Return its
output with findings first and in the severity order Grok provides. Preserve
uncertainty and residual-risk statements. If Grok is unavailable, direct the
user to `$grok-setup`.
