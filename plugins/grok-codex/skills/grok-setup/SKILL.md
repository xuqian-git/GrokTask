---
name: grok-setup
description: Check whether Grok Build is installed and locally authenticated for the grok-codex plugin. Use when configuring the plugin, diagnosing a failed Grok invocation, or when the user asks whether Grok is ready. Do not install software or perform an interactive login without a separate explicit request.
---

# Grok setup

Resolve `../../scripts/grok-companion.mjs` relative to this `SKILL.md`; do not
resolve it relative to the user's current working directory.

Run exactly one readiness check:

```bash
node "<resolved-plugin-root>/scripts/grok-companion.mjs" setup
```

Present the result plainly:

- If the binary is missing, point the user to `https://x.ai/cli` or explain
  that `GROK_BIN` can name a custom executable.
- If authentication is unknown, tell the user to run `grok login` in their own
  terminal or set `XAI_API_KEY`.
- If the check succeeds, confirm that the Grok delegation and review skills
  are ready.

Do not delegate a task during setup.
