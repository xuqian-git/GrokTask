---
name: grok-adversarial-review
description: Run a skeptical, read-only Grok Build review that assumes a change may be wrong and hunts for correctness, security, race-condition, error-handling, and edge-case failures. Use for high-risk changes or when the user explicitly asks for an adversarial or especially rigorous Grok review. Never edit files.
---

# Grok adversarial review

Run a review-only challenge pass. Do not apply fixes.

Resolve `../../scripts/grok-companion.mjs` relative to this `SKILL.md`. Invoke:

```bash
node "<resolved-plugin-root>/scripts/grok-companion.mjs" adversarial-review <flags> -- "<optional focus>"
```

Accept the same routing flags as `$grok-review`: `--base`, `--scope`, `--wait`,
`--background`, `--effort`, `--model`, and `--cwd`. Honor an explicit
execution mode; otherwise wait only for clearly small reviews and use
background mode for larger or uncertain scopes.

Return the companion output without softening, re-ranking, or inventing
findings. Preserve observed-versus-inferred distinctions and the residual-risk
statement. If Grok is unavailable, direct the user to `$grok-setup`.
