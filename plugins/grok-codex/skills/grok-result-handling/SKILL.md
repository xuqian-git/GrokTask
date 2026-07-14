---
name: grok-result-handling
description: Present Grok Build companion output faithfully after a delegated task or code review. Use when handling Grok results, failures, background log locations, review findings, session IDs, or residual-risk notes.
---

# Grok result handling

Preserve the companion's result semantics:

- Keep review findings first and in Grok's severity order.
- Preserve file and line locations exactly as reported.
- Keep facts, inferences, uncertainties, and residual risks distinct.
- State when a task made edits if Grok reports that it did.
- For a background run, surface the PID and log path without claiming the task
  is complete.
- For a failed or incomplete run, report the failure and stop. Do not silently
  substitute a Codex implementation or review.
- Never apply fixes while presenting a read-only review. Treat fixing findings
  as a separate user request.
